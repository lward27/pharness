# PHarness product and autonomy review

Date: 2026-09-04. Source revision: `12d36e97e7e31d01b3fcb7f1aedbb158d5f95c2d`. [Review overview](ASTRA-REVIEW-OVERVIEW.md) · [Evidence and limitations](ASTRA-FINDINGS-AND-EVIDENCE.md)

## The intended product and the product currently evidenced

The owner describes autonomous SDLC embedded in the environment that hosts a product. Taken seriously, that means PHarness should connect a desired product change to source work, validation, delivery, and observation of the resulting running product. An operator should supervise exceptions and authority boundaries without reconstructing this chain from separate tools.

The source supports important parts of that chain. The current contracts, however, deliberately define a narrower repository-only mode. This is a legitimate development boundary. It becomes confusing when that boundary is obscured by broader product language or a deployment-oriented interface.

| Part of the intended experience | Evidence in this checkout | What the evidence does not establish |
| --- | --- | --- |
| Describe a desired change | Durable WorkItems, pinned source and acceptance inputs, operator state | That first use is straightforward for someone other than the author |
| Understand and prepare a repository | Product/repository APIs, deterministic discovery, onboarding, readiness and exact-revision contracts | A completed current UI path through those capabilities |
| Execute bounded coding work | Isolated attempts, scoped tools, role-specific profiles, Implement → Test → Verify continuation | Reliable success across representative real product changes |
| Decide from evidence | Immutable stage outcomes, validations, context packs, effective outcome pointers | That every normalized conclusion faithfully summarizes the underlying claims; see F02 |
| Deliver reviewed source | SourceDeliveryIntent, dedicated writer/observer paths, required-check and exact-head validation | Frictionless closure; F01 reproduces a failure after normal prior state |
| Release into hosting | Existing typed Tekton, GitOps, Argo and release-evidence paths | A unified released Repo Mode journey through deployment |
| Verify the running product | Runtime observation and verification capabilities in the connected path | A continuous autonomous product stewardship loop demonstrated end to end |

Evidence: [Repo Mode product contract](../../design/repo-mode-v1-product-contract.md), [product implementation](https://github.com/lward27/pharness/blob/12d36e97e7e31d01b3fcb7f1aedbb158d5f95c2d/crates/pharness-api/src/app/products.rs), [Repo Mode controller](https://github.com/lward27/pharness/blob/12d36e97e7e31d01b3fcb7f1aedbb158d5f95c2d/crates/pharness-api/src/app/repo_mode.rs), [worker outcome handling](https://github.com/lward27/pharness/blob/12d36e97e7e31d01b3fcb7f1aedbb158d5f95c2d/crates/pharness-api/src/worker.rs), and [typed dispatch](https://github.com/lward27/pharness/blob/12d36e97e7e31d01b3fcb7f1aedbb158d5f95c2d/crates/pharness-api/src/dispatch.rs).

## Autonomy needs a precise boundary

**Objective:** Repo Mode closes after observing manual source merge. Its Release and Observe stages are explicitly inapplicable. Automatic merge, deployment, whole-attempt retry, and rollback are out of the V1 product contract. The existing chain can continue between authorized coding stages; not every transition requires a fresh human click. It would be inaccurate to call the entire system manual, just as it would be inaccurate to call the complete hosting lifecycle autonomous.

The wait reconciliation endpoint is also narrower than its name might suggest: it reconciles due waits from persisted evidence; it does not itself run the provider or Job that produces the observation. Its existence alone is not proof of a continuously operating observer. See [wait reconciliation](https://github.com/lward27/pharness/blob/12d36e97e7e31d01b3fcb7f1aedbb158d5f95c2d/crates/pharness-api/src/app/work_items/waits.rs).

**Judgment:** The product should explain autonomy as a bounded promise: “PHarness can do these steps under this authorization; it stops here for this reason.” “Autonomous” without that boundary creates a larger expectation than the current evidence supports.

This does not call for removing manual approval, enabling automatic merge, or adding a scheduler. It calls for accurately presenting the behavior that already exists. A source-only WorkItem can be a useful completed outcome while explicitly saying that nothing has been deployed.

## The strategic inconsistency is real

The [product vision](../../design/product-vision-and-boundaries.md) describes repository-first commercial value through a central deployment, with connected mode and customer-side satellite work deferred. The owner's current framing emphasizes embedding the SDLC inside product hosting. These directions can coexist over time, but they imply different first users, first screens, prerequisites, and meanings of completion.

**Judgment:** PHarness is paying the complexity cost of both directions before making either approach obvious. The next usability work should not quietly decide this strategic question by adding more navigation. Document one primary context for the experience being refined and explicitly bound the other. This is a positioning decision, not a request to build either missing mode.

For this review, the owner's hosting-centered goal is the assessment target. The repository-only contract is treated as the current bounded implementation direction, not as evidence that the intended goal has changed.

## A useful internal model is becoming too much public vocabulary

Product, Repository, WorkItem, Run, StageExecution, StageOutcome, WorkPlan, ChangeSet, SourceDeliveryIntent, PipelineIntent, Release, Observation, RemediationPlan, ToolApproval, and ApprovalGate each have plausible engineering purposes. The mistake would be making the operator learn all of them before completing ordinary work.

The product already has enough structure to support a simpler reading path:

| Operator question | Existing information that can answer it | Detail to reveal when relevant |
| --- | --- | --- |
| What change are we trying to make? | WorkItem title, intent, acceptance, target | Pinned product/repository snapshot |
| What is happening now? | Current operator state and effective stage outcome | Run, StageExecution, context pack |
| Why does this need me? | Current blocker, wait, or review requirement | Validation, policy, exact source and authority binding |
| What will this action do? | Existing action preview and authorization scope | External-effect executor, immutable material hash |
| What happened, and did it meet the goal? | Acceptance and source/release outcome | Full artifacts, logs, audit history |

This is presentation of existing information. It does not require collapsing the database model or inventing a new “unified workflow” resource.

## Keep the boundaries that protect comprehension

Some distinctions should remain explicit because they change a decision:

- **A tool approval authorizes one execution action. A lifecycle decision records whether a review condition is met.** Combining their authorization semantics would make the system less understandable, not simpler.
- **Merged source, deployed release, and verified product behavior are different outcomes.** A single green “done” label cannot substitute for them.
- **A Product can contain concurrent WorkItems.** It does not have one synthetic lifecycle stage or a meaningful universal autonomy percentage.
- **Current outcome and historical attempts serve different purposes.** Show the current answer first, but retain the history needed to understand retries and supersession.
- **Capability, policy permission, and current authorization are different.** The UI can explain these in ordinary language without exposing three competing dashboards.

These principles are already substantially present in the [product model](../../design/product-model.md), [stage outcome contract](../../design/stage-outcomes-and-evidence-handoffs.md), and [UI guidance](../../../ui/AGENTS.md). The opportunity is to apply them consistently.

## What I would simplify

**One authoritative explanation at a time.** The selected prototype's six-stage rail, the seven-stage product lifecycle, and the implemented console's older delivery surfaces should not all describe one workflow without a mapping. Choose the applicable existing state sequence and use that same language in the UI and docs. Hide or plainly mark inapplicable stages rather than making them look like future work waiting to run.

**One primary action for the current condition.** “Blocked” should lead to the relevant existing review or recovery action. “Waiting on GitHub” should explain the dependency and freshness of the last observation. “Running” should need no ritual clicks to continue already-authorized internal work. None of this requires a general action recommendation engine.

**One source of configuration truth.** The exact source revision and executable repository contract are important safety inputs. Ordinary users should encounter them as understandable reviewed context, not repeated manual form work. Where existing validated configuration is available, reuse it; where it is unavailable, say so and ask only for the missing information.

**One clearly bounded definition of success.** The acceptance language should describe the desired change. The closing screen should then say which acceptance evidence exists and whether the boundary was source merge, release, or runtime verification. Retain technical evidence below that statement.

## What I would not add

I would not add a workflow builder, generalized multi-agent customization, an autonomy score, another fleet dashboard, or a universal resource abstraction to solve these findings. I would not replace SQLite or split services merely because the schema is large. I would not expand the screen contract into every envisioned destination as part of a usability cleanup.

**My candid view:** the architecture has earned a clearer product story. It has not yet earned a claim that the complete autonomous hosting lifecycle is dependable. The fastest way to close that credibility gap is a narrower, understandable experience backed by truthful completion evidence, not a larger promise.
