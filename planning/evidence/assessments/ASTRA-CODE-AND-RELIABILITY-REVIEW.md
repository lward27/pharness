# PHarness code and reliability review

Date: 2026-09-04. Source revision: `12d36e97e7e31d01b3fcb7f1aedbb158d5f95c2d`. [Overview](ASTRA-REVIEW-OVERVIEW.md) · [Detailed findings and reproduction](ASTRA-FINDINGS-AND-EVIDENCE.md)

## Overall judgment

The code is serious, increasingly structured control-plane work. Its strongest quality is that it makes authority and evidence explicit instead of assuming a successful model response is a successful product change. Its weakest quality is the distance between those carefully stated invariants and the seams where several controllers update related durable state.

The right response is to repair the specific seams and simplify ownership. A rewrite, another service boundary, or a generalized workflow engine would add risk without resolving the demonstrated problems.

## What is solid

The nine-crate workspace separates domain/runtime concerns, configuration, model integration, storage, attempt hosting, API orchestration, workers, CLI, and evaluation. The API is composed from feature modules rather than the older single giant `app.rs`.

The source has important safety and reliability mechanisms:

- Exact source revisions, pinned input snapshots, state versions, and material hashes bind decisions to the data reviewed.
- Stage outcomes are immutable, with an explicit current/effective outcome pointer instead of rewriting history.
- Tool execution, source writing, source observation, pipeline execution, and GitOps effects have bounded ownership and purpose-specific execution paths.
- Repository verification checks command evidence and upstream outcomes rather than accepting a final narrative alone.
- The coding runtime contains context budgeting, output truncation, bounded recovery, and completion requirements. It can reject completion without meaningful workspace evidence.
- SQLite and the single API replica keep the deployment understandable. The Helm chart uses a single-writer configuration and Recreate strategy. That is a reasonable deliberate constraint for the present scope.

Evidence: [workspace](https://github.com/lward27/pharness/blob/12d36e97e7e31d01b3fcb7f1aedbb158d5f95c2d/Cargo.toml), [API composition](https://github.com/lward27/pharness/blob/12d36e97e7e31d01b3fcb7f1aedbb158d5f95c2d/crates/pharness-api/src/app/mod.rs), [dispatch](https://github.com/lward27/pharness/blob/12d36e97e7e31d01b3fcb7f1aedbb158d5f95c2d/crates/pharness-api/src/dispatch.rs), [stage model](https://github.com/lward27/pharness/blob/12d36e97e7e31d01b3fcb7f1aedbb158d5f95c2d/crates/pharness-core/src/repo_mode.rs), [stage storage](https://github.com/lward27/pharness/blob/12d36e97e7e31d01b3fcb7f1aedbb158d5f95c2d/crates/pharness-store/src/sqlite/repo_mode.rs), [context management](https://github.com/lward27/pharness/blob/12d36e97e7e31d01b3fcb7f1aedbb158d5f95c2d/crates/pharness-core/src/agent/context.rs), [runtime](https://github.com/lward27/pharness/blob/12d36e97e7e31d01b3fcb7f1aedbb158d5f95c2d/crates/pharness-core/src/agent/runtime.rs), [API deployment](https://github.com/lward27/pharness/blob/12d36e97e7e31d01b3fcb7f1aedbb158d5f95c2d/deploy/helm/pharness/templates/api.yaml).

These strengths deserve credit. Earlier reviews that described missing context/recovery machinery or an undecomposed API should not be reused as current findings.

## F01 — Source-delivery closure duplicates already sealed stages

**Classification: confirmed defect, high priority within the Repo Mode feature path.**

The normal Repo Mode audit helper also seals Release and Observe as inapplicable. Source writer dispatch invokes this audit helper. Later, successful merge observation calls `seal_source_delivery_closure`, which writes the source-delivery outcome and unconditionally creates Release and Observe at sequence 1 again.

The database enforces uniqueness on WorkItem, stage key, and sequence. When the tail already exists, closure returns an API 500 with a uniqueness failure before the source intent and WorkItem receive their final status updates.

The existing success fixture bypasses the earlier writer/audit path. In an isolated copy of the exact revision, the test was seeded through the actual existing tail-sealing helper before processing its provider observations. The otherwise existing success test then failed with:

```text
UNIQUE constraint failed: stage_executions.work_item_id,
stage_executions.stage_key, stage_executions.sequence
```

Evidence: [writer audit](https://github.com/lward27/pharness/blob/12d36e97e7e31d01b3fcb7f1aedbb158d5f95c2d/crates/pharness-api/src/app/repo_mode.rs#L1239), [closure](https://github.com/lward27/pharness/blob/12d36e97e7e31d01b3fcb7f1aedbb158d5f95c2d/crates/pharness-api/src/app/repo_mode.rs#L2225), [duplicate insertion](https://github.com/lward27/pharness/blob/12d36e97e7e31d01b3fcb7f1aedbb158d5f95c2d/crates/pharness-api/src/app/repo_mode.rs#L2315), [audit side effect](https://github.com/lward27/pharness/blob/12d36e97e7e31d01b3fcb7f1aedbb158d5f95c2d/crates/pharness-api/src/app/repo_mode.rs#L2362), [store insertion](https://github.com/lward27/pharness/blob/12d36e97e7e31d01b3fcb7f1aedbb158d5f95c2d/crates/pharness-store/src/sqlite/repo_mode.rs#L257), [existing closure test](https://github.com/lward27/pharness/blob/12d36e97e7e31d01b3fcb7f1aedbb158d5f95c2d/crates/pharness-api/src/app/tests/repo_mode_v1.rs#L339).

The source-delivery outcome is written before the error. A later repeat may take the existing-outcome path and recover. This review does not claim permanent corruption or failure on every retry. The defect is partial completion and an avoidable error during a normal transition.

**Small correction direction:** give tail sealing one idempotent owner and reuse it, rather than duplicating insertion logic. Make the completion path safe when prior work exists. The diagnostic should become a real journey/seam regression check when implementation is authorized.

There is a maintainability lesson here: a function called `append_repo_audit` has a non-obvious domain mutation. Moving or clearly naming that responsibility would make this code easier to reason about. That is a focused ownership correction, not a general event architecture proposal.

## F02 — Verification normalization can hide the meaning of reported risks

**Classification: high-confidence source finding; operator consequence inferred, not live-reproduced.**

`seal_repo_verify_stage` considers verification passed when the run is completed, the submission's decision is `approved`, and the effective Implement and Test outcomes succeeded. It does not use the verifier's submitted evidence references, contradictions, or risks to determine that result.

The entire submission is retained under `agent_claims`, which is good. However, the normalized outcome writes empty top-level risks and, on success, empty top-level contradictions. The corresponding validation is labeled valid. A submission containing an approved decision together with caveats can therefore produce a cleaner controller summary than the submission warrants.

Evidence: [pass condition](https://github.com/lward27/pharness/blob/12d36e97e7e31d01b3fcb7f1aedbb158d5f95c2d/crates/pharness-api/src/worker.rs#L761), [retained claims and normalized fields](https://github.com/lward27/pharness/blob/12d36e97e7e31d01b3fcb7f1aedbb158d5f95c2d/crates/pharness-api/src/worker.rs#L851), [structured submission handling](https://github.com/lward27/pharness/blob/12d36e97e7e31d01b3fcb7f1aedbb158d5f95c2d/crates/pharness-runhost/src/lib.rs#L1056), [stage evidence contract](../../design/stage-outcomes-and-evidence-handoffs.md).

The structured submission handler checks that the payload is a bounded, nonempty object. That is not full semantic verification of its advertised schema. Upstream success checks still provide real protection; this finding does not say a verifier can make any arbitrary failed build pass, nor that the raw evidence is discarded.

**Small correction direction:** faithfully represent reported risks/contradictions in the existing normalized result as verifier-reported caveats, not newly verified facts, and validate the existing submission contract at the receiving boundary. Clarify whether “valid” means a syntactically valid decision, an evidence-consistent decision, or satisfied product acceptance. Do not add a new policy engine to repair misleading field semantics.

## F11 — The architecture check currently fails, so its guarantee is unavailable

Running `scripts/check-app-module-boundaries.sh` reported:

- `products.rs`: 3,959 lines, above the 3,500-line limit.
- `repo_mode.rs`: 4,777 lines, above the same limit.
- A wildcard import at `repo_mode.rs:4443`, inside the test module.
- A dependency-parser crash: its regex interpreted prose inside a Rust string as a `use` tree.

Evidence: [boundary script](https://github.com/lward27/pharness/blob/12d36e97e7e31d01b3fcb7f1aedbb158d5f95c2d/scripts/check-app-module-boundaries.sh), [dependency parser](https://github.com/lward27/pharness/blob/12d36e97e7e31d01b3fcb7f1aedbb158d5f95c2d/scripts/app-module-dependencies.py#L104), [dated graph](../../architecture/app-module-dependency-graph-2026-08-21.md).

The wildcard is a rule violation, not evidence of a production dependency cycle. The parser failure means the present graph was not successfully checked. Do not cite the old graph's acyclic conclusion as a fresh result.

**Judgment:** the decomposed architecture is improving, but its guardrail is no longer reliable. Repairing the existing checker and deciding whether its test-import rule is intentional is more useful than declaring the architecture clean or launching another broad decomposition effort.

## Size and concentration

The tracked Rust source inventory, excluding nested generated evaluation workspaces, contains 139 files and 115,406 lines, including tests. Size alone is not a defect. Concentrated responsibilities do affect how easily a contributor can find the owner of a behavior.

| File | Lines | Review implication |
| --- | ---: | --- |
| `crates/pharness-cli/src/main.rs` | 9,600 | CLI behavior and command wiring require substantial navigation |
| `crates/pharness-store/src/sqlite.rs` | 8,960 | Persistence ownership remains concentrated despite newer feature modules |
| `crates/pharness-worker/src/main.rs` | 4,905 | Worker setup and execution paths deserve careful ownership boundaries |
| `crates/pharness-api/src/app/repo_mode.rs` | 4,777 | Several lifecycle responsibilities meet here; F01 is a concrete seam problem |
| `crates/pharness-api/src/dispatch.rs` | 4,205 | Shared dispatch is a consequential change surface |
| `crates/pharness-api/src/app/products.rs` | 3,959 | Product/onboarding growth has exceeded the project's declared module limit |

I would not turn these counts into a refactoring backlog. Extract only where a demonstrated bug or confusing ownership makes a smaller unit useful. The UI's widespread permissive types and compressed JSX similarly make state-contract mistakes easier to miss; tighten the specific API/view boundaries involved in the review findings rather than rewriting all components.

## F12 — Generated evaluation workspaces are tracked as source

Forty tracked files under `crates/pharness-eval/target/pharness-evals/` account for approximately 308 KB. Nine are generated Rust files totaling 9,032 lines. The root ignore rule covers `/target/`, which does not cover this nested target directory.

**Why it matters:** source inventories and searches become noisier, and future evaluations can mix disposable execution output with intentionally preserved evidence. This review excluded those files from its source metrics.

**Correction direction:** distinguish retained evaluation evidence from generated workspaces, and ignore the latter. Preserve intentional evidence before any cleanup. There is no recommendation to delete all evaluation history.

## F13 — The evidence is promising but narrower than the autonomous product claim

All 408 top-level Rust tests passed. Fresh schema construction also passed. Those checks provide useful confidence in tested behavior; F01 demonstrates that they do not cover every ordinary sequence of state transitions.

The [August 14 coding evaluation](../../evidence/evaluations/pharness-coding-eval-candidate-2026-08-14.md) deserves credit for matched comparisons and honest failed gates. Its recovery-focused v1.6 result improved from 8/16 baseline successes to 16/16 candidate successes across eight fixtures with two attempts each. That is evidence of a specific improvement, not a general real-world task success rate. Its model/prompt/fixture conditions also predate the current prompt revision.

Context packing now exists, but it uses approximate token accounting and truncation/removal of older exchanges rather than durable semantic memory. Separate Builder, Tester, and Verifier profiles improve responsibility separation, but can share the configured model; role labels alone are not independent validation. Completion checks establish workspace evidence, not complete product correctness.

The acceptance target should remain a real bounded journey through the existing workflow, including repeated observation, interruption, failure, and source closure. Expanding the set of features before that journey is dependable would make the proof problem harder.
