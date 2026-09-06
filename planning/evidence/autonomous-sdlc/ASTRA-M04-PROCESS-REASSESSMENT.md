# ASTRA M04: Reassess the process before another qualification cycle

Reviewed: 2026-09-06. Source: main `04dbaeab51d9de939f0743d587c3aa3d10c21a8f`.
Status: evidence-backed recommendation for owner review; no revised acceptance gate, model switch or production authorization is implemented by this document. Runtime `19b0c55` remains deployed and unqualified; its already-running Verifier evaluation is separate from this assessment.

## Judgment

**A targeted redesign of the agent-facing process and its qualification is warranted. A wholesale rewrite of PHarness is not justified by the current evidence.** Some implementation choices are creating avoidable failure, and parts of the evaluation cannot measure the engineering judgment their names imply. More prompt clauses and broad reruns are a poor next investment.

Keep the gateway execution path, deterministic tests, immutable source/evidence records, capability enforcement, bounded correction, durable operations and production approval boundary. They are useful foundations, although their complete autonomous integration is still unaccepted. Refactor the boundary where a model must reconstruct controller state, comply with redundant metadata requirements or satisfy a brittle prose-based scorer.

I should have moved to this review earlier. We ran expensive stage matrices while protocol checks were missing the actual provider context boundary and several fixtures/scorers were inconsistent. Retaining failed evidence and refusing to weaken gates was correct; it did not make those repeated full runs an efficient diagnosis strategy.

## What the evidence establishes

| Area | Current evidence | Interpretation and confidence |
| --- | --- | --- |
| Provider context delivery | Source-19 Onboarding produced no accepted proposal in 24 cases after 79 submit attempts. MiniMax's inspected prompt retained only the first of six separate system sections; joining them retained all six. | Confirmed transport/context-delivery defect. This is not proof that MiniMax cannot reason about repository onboarding. |
| Context capacity | The same Onboarding report recorded zero compactions and zero context-budget failures, despite 329 turns and 1,716,688 prompt tokens across the full evaluation. | No evidence that context-window exhaustion caused that failure. More context capacity or a larger retry budget would not repair omitted system sections. |
| Tool/controller/evaluator alignment | Test Diagnosis retained 24 typed results but passed only 1/12 in each attempt. Twenty-one cases had evidence-reference failures; ten had classification failures, with overlap. The tool accepted strings that the scorer refused. | Confirmed interface/evaluator mismatch. Model quality is confounded by a contract that accepts a submission at one layer and rejects its reference semantics at another. |
| Planning evaluation | Planner passed 6/12 and 7/12. Nine rejected plans mentioned prohibited operations in prose, usually to forbid them. Two actually named out-of-scope paths. | Both scorer defects and real scope errors exist. We must distinguish them without turning English parsing into the authorization mechanism. |
| Verification evaluation | The current Verifier fixture explicitly supplies `expected_decision` and the expected marker. The scorer checks decision, evidence ID and marker presence. Earlier results were 1/24 and 2/24, with 43 evidence/marker mismatches. | High confidence that this tests protocol/evidence conventions, not independent discovery of defects. A future green result on this suite alone would still not qualify engineering judgment. |
| Actual code editing | On runtime `48c77b7`, Builder passed the frozen 24-task benchmark twice, first pass, with 8/8 per language stack and no reported hidden-test false passes or policy violations. Seeded Repair also passed 24/24 twice. | Meaningful evidence that the existing execution path can edit and repair bounded code. It does not prove Finance delivery, general coding reliability, or repair of a real failed Builder handoff. |
| Model reasoning | Some Test Diagnosis outputs invented causes from file/class names. Models have not been compared on the same clean, realistic stage inputs. | A real quality concern remains. Its size and whether a different model fixes it are not established. |

These are overlapping failure classes, not percentages of one root-cause pie. Current tests cannot support a defensible claim that a particular share is prompting, tooling or model capability.

## The process choices I would change

### Put machine bookkeeping in trusted code

The runner already knows the discovery ID and hash, yet the onboarding tool requires the model to reproduce them. A proposal can be lost at that boundary before its useful content is evaluated. Evidence selection matters; reproducing a controller-owned hash does not require intelligence.

Have the model submit its proposed configuration, reasoning, uncertainties and selected evidence references. The trusted adapter attaches the original run/discovery/source bindings. Where the model chooses among evidence, use stable catalog handles whose native records carry full identity and hashes. Preserve stale-source, preimage and provenance checks. Never silently replace a model's conflicting supplied identifier with the current one, or attach the latest state to an older result.

This simplifies the model's job while retaining the audit contract. It does not make model claims verified facts.

### Use one explicit context assembly contract

The current runner assembles base, repository, environment, map, stage and controller instructions as separate system messages, with further budget/recovery sections. Provider behavior differed at exactly that boundary.

Keep a versioned, inspectable context envelope with clear instruction/data boundaries. Validate its native serialization for initial execution, replay, recovery and checkpoint paths. The actual provider-facing envelope must preserve all required sections. Missing required context should be diagnosed before a costly stage matrix. Avoid solving this by appending more reminders or exposing an ever-growing internal state dump to the model.

The merged MiniMax transport fix is worth keeping and validating. It addresses a demonstrated defect. Conventional prompt wording is a secondary optimization after delivery and schema alignment are correct.

### Stop using prose as an execution policy

The new Planner negation recognizer is a local repair to an overbroad substring rule. It remains the wrong long-term center of this gate: it adds rules about verbs, negation, exceptions and punctuation to interpret what a plan may execute.

Validate proposed paths, named commands, acceptance coverage and effects as structured data. Enforce actual actions through the existing capability and workspace boundaries. Evaluate explanatory prose for unsupported claims and contradictory plans, but do not treat a warning that says “do not run X” as an attempted execution of X. Ambiguous executable intent should block with a precise field-level explanation, not require a growing language parser.

### Reduce independent model roles until they earn their complexity

The current registry spreads six roles across five primary models. That creates a substantial provider, prompt, schema and handoff matrix before one complete Finance journey is proven.

The execution runtime is already shared; keep it. My preferred direction is one consistent agent contract and task-state pattern for planning, implementation and bounded repair; deterministic discovery/test execution where facts are machine-derived; and a separately scoped verification pass. Separate controlled stage Runs may remain. The intended simplification removes unnecessary model work and handoff translation, not the audit stages or execution engine. Preserve the named lifecycle stages and durable evidence. Preserve independent verification and production approval. We do not need a separate long-running model process merely to translate known metadata or label a deterministic command failure.

Keep a common model as a controlled diagnostic baseline before optimizing model selection per role. Compare candidates on identical valid inputs, tools and budgets. Do not silently switch current registered policies or assume a model is suitable merely because its name suggests coding, reasoning or low cost. Whether a separate diagnostic model adds value is an empirical question; it is not established today.

## Replace the qualification sequence, preserving acceptance strength

1. **Validate the harness first.** Exercise real request assembly, tool schemas, native binding and field-level rejection diagnostics with known valid/invalid submissions. Ensure fixtures contain the source and command outputs they claim. Separate provider-envelope failures, invalid submissions, missing evidence and wrong engineering decisions in reports.
2. **Use small live canaries before a full matrix.** Cover one valid and one blocked onboarding case, a grounded plan, an actual failing command diagnosis and a blind verification case through the real gateway/worker path. Inspect retained evidence for the first failure before repeating a large batch. These are diagnostics, never replacement acceptance results.
3. **Make the engineering oracle blind.** Keep answer keys outside model-visible task/context/evidence. Give Verifier real source changes, test receipts and requirements. Check whether it recognizes a genuine semantic defect, missing coverage or a correct change without being told the verdict. Reuse the existing finite fixture machinery; do not build another evaluation platform.
4. **Exercise the connected loop.** Test plan → implement → deterministic test → actual failure diagnosis → one correction → test → independent verification with preserved workspace and evidence lineage. The successful seeded Repair benchmark does not substitute for this handoff proof.
5. **Freeze a release candidate, then run full qualification.** Preserve the 24-task coding benchmark, two independent runs, all global/per-stack thresholds, no hidden-test false passes, unchanged budgets and one correction. Version any justified stage-fixture/scorer changes explicitly; retain original failures. Keep the current exact-runtime activation gate until a separately reviewed compatibility change exists. Do not build a new qualification-cache architecture merely to shorten this review.

The current Verifier suite can remain useful as a labelled protocol regression. Its answer-containing fixtures must not become the semantic acceptance oracle. Revised stage suites need documented mappings to the original scenarios and explicit owner review; changing the measurement must strengthen validity, not make a bad score disappear.

## Refactor boundary and decision test

**Refactor now:** agent-facing submissions, context assembly/validation, grounded stage fixtures, error diagnostics and the order of qualification. Evaluate consolidation of metadata-only model work and model selection using the smaller connected loop.

**Retain:** Rust service, SQLite/single writer, existing gateway and worker execution, durable stage/effect records, deterministic testing, immutable delivery evidence, limits, human production approval and bounded recovery policy. Their end-to-end gates remain open. Native Codex-host expansion and generic platform adapters remain deferred.

A larger replacement becomes justified if the simplified, correctly instrumented loop still cannot complete a representative bounded change under the existing limits with an explicitly selected capable control model, or if preserving exact evidence/recovery requires unavoidable duplicate authorities throughout the implementation. We have not established either condition. Current results instead show fixable interface/measurement defects alongside real but unisolated model-quality risks.

No additional broad qualification run, model change or production action was initiated for this reassessment. The already-running source-19 Verifier job may finish and retain its evidence. The next M04 implementation should follow this bounded redesign after the owner reviews the direction, rather than continue accumulating prompt/scorer exceptions.

## Evidence and source pointers

- [Source-19 Onboarding failure and context diagnostic](ASTRA-M04-19B0C55-ONBOARDING-FAILURE.md); [merged transport correction](ASTRA-M04-SYSTEM-CONTEXT-DELIVERY.md).
- [Source-19 Planner analysis](ASTRA-M04-19B0C55-PLANNER-FAILURE.md); [Test Diagnosis analysis](ASTRA-M04-19B0C55-TEST-DIAGNOSIS-FAILURE.md); [latest contract correction and its limits](ASTRA-M04-STAGE-EVIDENCE-CONTRACT.md).
- [Builder qualification](ASTRA-M04-48C77B7-BUILDER-QUALIFICATION.md); [seeded Repair qualification](ASTRA-M04-48C77B7-REPAIR-QUALIFICATION.md); [earlier Verifier failure and protocol-only limitation](ASTRA-M04-48C77B7-VERIFIER-FAILURE.md).
- Agent-facing schemas and prompts: `crates/pharness-runhost/src/prompt.rs`; context assembly and discovery validation: `crates/pharness-runhost/src/lib.rs`.
- Model-visible fixture construction and Verifier oracle: `crates/pharness-eval/src/stage_suites.rs`; Planner/Diagnosis predicates: `crates/pharness-eval/src/stage_suites/integrity.rs` and `integrity/planner_boundary.rs`.
- Registered policies: `deploy/helm/pharness/files/inference-registry.json`; exact-runtime activation: `crates/pharness-api/src/app/hosted_workflow.rs`.

This review uses code and retained execution evidence. It launches no new model comparison and cannot rank general model capability from these confounded stage scores.
