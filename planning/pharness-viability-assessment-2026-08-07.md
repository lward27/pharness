# Pharness Viability Assessment

Date: 2026-08-07

## Bottom Line

Pharness is a credible and unusually safety-conscious SDLC control plane, but
it is not yet a proven general-purpose coding-agent harness. It can already
run bounded Fireworks tool loops, inspect and change an isolated workspace,
pause and resume exact actions through approvals, capture durable evidence,
and coordinate strongly typed delivery stages. Those are real capabilities,
not a mock architecture.

The present coding loop should be expected to handle small, clear changes. It
has not yet earned confidence on sustained, multi-file implementation work.
The main risks are not the short system prompt. They are weak context
management, fatal handling of recoverable tool errors, coarse large-file
navigation, limited automated acceptance checking, and the absence of a
representative coding evaluation suite.

Pharness can succeed, but success now depends on shifting effort from adding
more control-plane surface area to proving and hardening the inner coding loop,
while decomposing the largest modules before they become the permanent
architecture.

## Codebase Shape

The current Rust workspace contains roughly 75,800 lines of Rust. Its largest
files are approximately:

- `crates/pharness-api/src/app.rs`: 35,000 lines, with production code through
  about line 23,000 and a large in-module test suite after that.
- `crates/pharness-cli/src/main.rs`: 9,600 lines.
- `crates/pharness-store/src/sqlite.rs`: 7,800 lines.
- `crates/pharness-core/src/tools/cluster.rs`: 3,350 lines.

A 35,000-line Rust module is not normal or healthy merely because it compiles.
The compiler can type-check it, and the in-module tests can protect behavior,
but the file concentrates routing, authentication, workflow transitions,
reconciliation, policy-adjacent checks, persistence orchestration, delivery
logic, and tests behind one module boundary. That increases review cost,
merge-conflict pressure, accidental coupling, and the difficulty of proving
local invariants.

Under technical scrutiny, the response should be candid: the product logic is
substantial and tested, but `app.rs` is a god module and is architectural debt.
The same criticism, at a smaller scale, applies to the CLI and SQLite adapter.
This does not invalidate Pharness. It means modular decomposition is now
product-risk reduction, not cosmetic cleanup.

## What Is Strong

1. **The safety model is concrete.** Actions are typed, policy is evaluated
   before execution, dangerous categories fail closed, approvals retain the
   exact reviewed action, and resume continues from a durable transcript.

2. **Isolation and provenance are first-class.** Workspaces are pinned to
   source revisions, remote repositories are allowlisted, Kubernetes workers
   are bounded, and Git/Tekton/GitOps/Argo responsibilities are separated by
   identity and capability.

3. **The control plane is durable.** Runs, events, approvals, artifacts,
   observations, incidents, plans, waits, and delivery evidence are modeled as
   durable records rather than being hidden inside an ephemeral chat.

4. **The delivery architecture is more disciplined than most agent demos.**
   It distinguishes code generation from Git delivery, build execution,
   GitOps mutation, deployment, and verification. Explicit apply boundaries
   and exact-target preflights are meaningful strengths.

5. **There is significant automated coverage.** On 2026-08-07, the current
   dirty worktree passed all 287 workspace tests. Existing live records also
   demonstrate Fireworks native tool calls, approval/resume, bounded writes
   and patches, read-only cluster operations, and Tekton execution/evidence.

## What Is Not Yet Strong Enough

### 1. Recoverable mistakes can terminate a run

The runtime continues when a tool returns a structured error result, but a
`ToolExecutor` error immediately fails the run. Missing files, invalid paths,
ambiguous patches, timeouts, and other ordinary agent mistakes therefore have
paths that prevent the model from seeing the error and correcting itself.
Strong coding agents must be able to recover from routine bad assumptions.

### 2. There is no real context-management strategy

Every turn resends the accumulated message transcript. There is no visible
token budgeting, summarization, compaction, retrieval of prior relevant
context, or deliberate eviction of large tool outputs. A single file read may
return 256 KiB and shell stdout and stderr may each reach 512 KiB. A long run
can therefore exhaust the provider context even when the model is behaving
reasonably.

### 3. Repository navigation is too coarse for large codebases

`read_file` reads only from the start of a file and has no offset or line-range
arguments. Search is bounded but basic. An agent can fall back to shell tools,
but that path is less typed and some ordinary commands are classified as
unknown. Pharness's own `app.rs` is a good adversarial example: the native file
tool cannot navigate the whole module precisely.

### 4. The edit-test loop is possible but not yet excellent

`write_file`, exact replacement through `patch_file`, and `run_shell` are
enough to make changes and run tests. They are not yet enough to make the loop
robust. Exact replacement is brittle, filesystem tool failures may terminate
the run, test commands may introduce approval friction depending on policy,
and there is no dedicated diagnostic or test-result abstraction.

### 5. Success is substantially self-reported

The model can call `finish(success=true)`. Pharness captures diffs and shell
events and later places human and delivery gates around them, which is good,
but the inner run does not itself enforce acceptance criteria, require a test
attempt, or independently judge whether the requested change is correct.

### 6. Coding quality has not been measured on representative tasks

The recorded model-backed wins are important plumbing proofs, but the visible
coding smokes are intentionally trivial, such as creating one Markdown file.
There is no repeatable suite of multi-file bugs/features with known tests,
trajectory capture, failure classification, and pass-rate comparison. Until
that exists, claims about autonomous coding quality are hypotheses.

### 7. Product breadth is outrunning the core agent loop

Pharness has built a large amount of SDLC domain machinery. Much of it is
thoughtful, but the product risks becoming an excellent governance and
evidence system wrapped around a mediocre coding loop. The next gains should
come from agent reliability and measured task completion, not another large
set of workflow states or endpoints.

## The System Prompt

The prompt's size is not concerning. Large commercial-agent system prompts
often contain product-wide UI rules, security policy, tool instructions,
collaboration behavior, formatting constraints, and compatibility guidance.
Pharness has a narrower worker role, and its tool schemas carry additional
instruction. A short, precise prompt can outperform a long one.

The current prompt is reasonable for the implemented alpha, but it is too
generic to carry a mature coding agent by itself. More useful than simply
making it longer would be dynamic context: repository instructions, task and
acceptance state, available budget, current plan, test conventions, prior
failures, and explicit guidance about recovering from tool errors. Those
inputs should be structured and selectively injected rather than accumulated
as permanent prose.

## Can A Fireworks Agent Actually Write And Iterate?

Yes, for bounded tasks. The loop has the necessary minimum mechanics:
inspection, search, writes, patches, shell execution, Git diff/status,
multi-turn native tool calls, approval resume, and final evidence capture.

For nontrivial coding work today, reliability is likely to be inconsistent.
The model may complete a clean small change, but it can also fail from an
ordinary tool mistake, lose effectiveness as its context grows, struggle to
navigate large files, or declare success without sufficient verification.
That is an alpha coding agent, not yet an unattended software engineer.

The Fireworks provider is not the architectural blocker. Model quality matters,
but changing models or enlarging the prompt will not compensate for weak
recovery, context control, navigation, verification, and evaluation.

## Honest Verdict

- **As a typed, auditable SDLC control plane:** strong and differentiated.
- **As a safety architecture for agent-driven delivery:** promising and in
  several areas genuinely good.
- **As a maintainable Rust codebase:** functional, but the largest modules
  would draw justified criticism and need decomposition.
- **As a coding-agent harness today:** viable alpha, not proven production
  quality and not yet competitive with mature coding harnesses on iteration.
- **Chance of success:** real, provided the project confronts these weaknesses
  directly. Continuing to add breadth without that correction would lower the
  chance materially.

## Recommended Priority Order

1. Build a representative coding evaluation suite and publish the baseline,
   including failure categories and full trajectories.
2. Make ordinary tool failures recoverable by returning structured feedback to
   the model under a bounded retry policy.
3. Add explicit context budgeting, compact tool outputs, transcript
   summarization, and continuation semantics.
4. Improve code navigation and editing with ranged reads, bounded search, and
   better patch/application feedback.
5. Enforce task-level verification: acceptance criteria, changed-path checks,
   required test evidence when applicable, and an independent completion
   decision.
6. Decompose `app.rs`, then the CLI and SQLite adapter, around domain services
   and narrow interfaces while preserving existing behavior tests.
7. Resume broader autonomous-delivery expansion only after nontrivial coding
   tasks show a stable, measured pass rate.

The core idea is worth pursuing. The right reaction to the current size is not
to abandon it, and not to excuse it. It is to recognize that Pharness has
successfully built much of the safe outer control plane and now needs to prove
and strengthen the inner intelligence loop that makes the control plane useful.
