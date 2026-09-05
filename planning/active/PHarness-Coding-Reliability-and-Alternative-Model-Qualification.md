# PHarness Coding Reliability and Alternative-Model Qualification Milestone

> Current status (2026-09-04): **Reference for ASTRA M04**.
> Replacement/authority: [ASTRA](../programs/autonomous-sdlc/ASTRA-04-CODING-RELIABILITY-QUALIFICATION.md).
> The existing frozen gates remain binding. This document is a procedure reference, not an independent expansion program.

Original status: active
Baseline: `07b47b91e0c3eca4d18bde112e0549d12587b98a`
Priority: correctness of produced code before further platform expansion

## Current acceptance status (2026-08-31)

V2.2 implementation `1423a404d6e28fdcda48e8601332f509677c87a9` is
deployed through release pin `1a1f57191932a8a1eee3a3904f42e8e4a8d4b5e1`
with the feature disabled. Deterministic coding, repair, and stage replay suites
all pass from the exact evaluator image. Provider-backed qualification and the
disposable repair WorkItem remain incomplete because Fireworks is returning
account-level `PRECONDITION_FAILED`. See the
[dated evidence record](../evidence/evaluations/coding-reliability-v22-release-and-replay-2026-08-31.md).

## Objective

Make PHarness reliably produce, test, repair, and verify repository changes before investing further in security, audit, approval, Connected Mode, or deployment features. The inference gateway is treated as transport infrastructure, not proof of coding quality.

The acceptance bar is a frozen 24-task Rust/Python/Node benchmark with at least 21/24 first-pass successes and 23/24 successes after one bounded correction, with per-stack floors of 6/8 and 7/8 respectively. Hidden-test false passes, undeclared mutations, environment rediscovery loops, network/package-install activity, and Git mutations are all zero-tolerance failures.

## Locked behavior

- Optimize correctness before cost or latency.
- Use deterministic acceptance execution before an optional model Test Diagnoser.
- Allow at most one automatic repair execution on the same durable workspace.
- Give Builders only typed atomic patching and bounded offline command execution.
- Bind immutable prompt, tool-schema, context-policy, protocol-calibration, target, policy, profile, and runtime hashes to every V2 execution.
- Keep legacy policies and WorkItems readable and runnable for rollback.
- Keep manual source-PR merge and immutable source pinning.
- Freeze unrelated platform maturity work.

## Runtime and prompt contract

V2 uses a small common runtime prompt plus immutable prompt packs:

- `repo-onboarding-v2`
- `repo-planner-v2`
- `repo-builder-v2`
- `repo-repair-v2`
- `repo-test-diagnoser-v2`
- `repo-verifier-v2`

New reasoning policies use `ToolChoiceMode=auto`; legacy policies may retain `required`. Parallel tool calls remain disabled. Typed stage submissions are terminal after controller validation. Missing, malformed, multiple, or invalid actions are non-executing protocol errors with at most two corrective reprompts per Run.

Preparation seals a deterministic repository map. Each turn receives a controller-derived execution ledger. Context accounting includes prompts, tool schemas, context packs, reasoning replay, and tool output. Deterministic checkpoints preserve mandatory intent, acceptance, failure, decision, and provenance state during compaction.

## Builder interface

`apply_patch` accepts one unified diff and validates every preimage hash and writable boundary before making an all-or-nothing mutation.

`run_workspace_command` accepts executable, argv, repository-relative cwd, and timeout. The executor derives allowed executables from the EnvironmentSnapshot and RepositoryContract; it denies shell evaluation, chaining, redirection, subshells, network tools, package installation, inline interpreted programs, Git mutation, and undeclared environment changes. Bounded model feedback and a durable full-output artifact are recorded.

The typed acceptance executor remains the final authority.

## Deterministic Test and correction controller

1. Builder submits an implementation.
2. The controller runs every selected RepositoryContract acceptance command.
3. Evaluation fixtures also run evaluator-owned hidden checks.
4. Passing evidence seals a controller-origin Test outcome.
5. A repairable failure may be diagnosed by the optional Test Diagnoser.
6. One Repair Builder may modify the same workspace.
7. The complete deterministic Test stage reruns.
8. A fresh Verifier reviews the final workspace.

A Verifier rejection may consume the same single correction allowance if Test did not. Structural failures such as checkout, runner, dependency, contract, environment, authorization, capability, or active-time failures block for operator action. Source delivery stays unavailable until the final deterministic Test and Verifier outcomes succeed.

## Candidate policies

| Stage | Primary | Challenger/reference |
|---|---|---|
| Onboarding | MiniMax M3 | Kimi K3 |
| Planner | Kimi K3 | MiniMax M3; GPT-5.5 reference |
| Builder | Kimi K2.7 Code | DeepSeek V4 Pro |
| Repair | Kimi K3 | GPT-5.5 reference |
| Test diagnosis | Nemotron Lightning | MiniMax M3 |
| Verifier | GLM-5.3 | Kimi K3; GPT-5.5 reference |

GPT-5.5 is an OpenRouter quality reference with an exact OpenAI provider route, fallback disabled, and required parameter support. Qwen 3.8 remains protocol-calibration-only until its request rejection is resolved. No local-model qualification or fine-tuning is part of this milestone.

## Qualification sequence

1. Run ten protocol cases three times each and require 30/30.
2. Classify every failure as provider/transport, protocol, model inability, policy rejection, tool failure, acceptance/hidden-test failure, missing evidence, semantic false approval/rejection, or budget exhaustion.
3. Run two independent attempts over the frozen 24-task code suite.
4. Run 12-case Onboarding and Planner suites, deterministic Test cases, and 24 Verifier patches.
5. Compare models only on identical harness revisions.
6. Tune only the two strongest candidates, then rerun an untouched holdout.
7. Promote only the exact immutable combination that passed.

Cost, latency, turns, and token usage are reported but cannot offset a correctness failure.

## Rollout

1. Implement behind `codingReliabilityV2.enabled=false`.
2. Run deterministic unit, integration, legacy-compatibility, UI, Helm, and manifest gates.
3. Build API, UI, Python runner, Node runner, gateway, and evaluator from one merged SHA on `lucas-desktop` with no automatic fallback.
4. Pin every Linux/AMD64 digest in a separate GitOps release commit and deploy V2 disabled.
5. Run protocol calibration and the frozen offline benchmark.
6. Enable V2 for one disposable supervised WorkItem, including a deterministic failure and repair.
7. Resume Finance FRC-2 through FRC-6 sequentially only after the offline gate passes.
8. Promote passing stage policies independently; retain all failures as evidence.

## Required acceptance

- `cargo fmt --check`
- `cargo test --workspace`
- workspace/all-target Clippy with warnings denied
- UI build, Vitest, Playwright compatibility, and accessibility checks
- Helm lint/template/schema and Kubernetes server-side dry-run
- no deployed `:latest`
- Linux/AMD64 OCI verification
- prompt/hash determinism
- dynamic tool-schema validation
- atomic patch and bounded-command policy tests
- context accounting and compaction tests
- deterministic Test and one-correction controller tests
- legacy Run and WorkItem compatibility
- protocol, stage, frozen-code, disposable-live, and Finance acceptance gates

This milestone does not add swarms, parallel Builders, automatic replanning, repeated repairs, automatic merge, deployment expansion, local-model qualification, fine-tuning, or unrelated governance features.
