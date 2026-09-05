# PHarness documentation and approachability review

Date: 2026-09-04. Source revision: `12d36e97e7e31d01b3fcb7f1aedbb158d5f95c2d`. [Overview](ASTRA-REVIEW-OVERVIEW.md) · [Evidence](ASTRA-FINDINGS-AND-EVIDENCE.md)

## The main problem is competing truth, not missing documentation

PHarness has substantial documentation: product principles, screen contracts, ADRs, implemented milestones, operational playbooks, evaluations, diagrams, UI QA records, and an operator handbook. The problem is not a lack of thought. It is that a new reader must determine which version of the project each artifact describes.

**Judgment:** the documentation is better at recording development than introducing the product. An experienced contributor can piece the story together. A first-time user or a fresh coding agent is likely to take a confident but outdated statement literally.

This review should itself remain dated evidence. Its findings should not become a new competing product contract.

## What is good

- The planning tree distinguishes active, implemented, operational, evidence, design, and archived material. That is a useful structure.
- Product and screen contracts contain strong invariants: server-owned state, pinned identity, one WorkItem's lifecycle, current versus historical outcomes, and honest unavailable capabilities.
- Operational documents are generally bounded around actual validation tasks rather than treating a deployment as automatic acceptance.
- The coding evaluation records preserve failed comparisons and explain why a later comparison is eligible. This is unusually useful evidence.
- The untracked `living-docs` handbook has a more approachable operator entry point and explicitly distinguishes local use, isolated coding alpha, Repo Mode preview, and guarded connected capabilities.

Evidence: [documentation map](../../README.md), [design index](../../design/README.md), `living-docs/README.md` (untracked input retained in the original reviewed checkout), [coding evaluation](../evaluations/pharness-coding-eval-candidate-2026-08-14.md).

## F08 — The entry points describe different present-tense products

| Artifact | What it says or implies | Conflict in this checkout | Smallest useful correction |
| --- | --- | --- | --- |
| Root README opening and V1 scope | Lightweight local Fireworks harness; no remote execution; Kubernetes workers are future direction | Later sections and executable code already contain cluster execution and typed delivery capabilities | Replace the opening with a short current capability/boundary statement; move historical phase descriptions out of the first-use path |
| Root README coding gate | Do not begin `app.rs` extraction until a comparison passes | The README's own status section says decomposition is complete | Mark the gate as historical evidence instead of a current instruction |
| Planning index and active index | No active implementation milestone; Repo Mode product/screen work awaits planning | `active/PHarness-Repo-Mode-V1-Product-and-Control-Plane.md`, migrations, controllers, and fixtures exist | State which milestone the branch is implementing and what remains unaccepted; distinguish it from the stable release |
| Product vision and Repo Mode contracts | Repository-first central mode; connected/satellite work later | Owner's review target is hosting-embedded autonomous SDLC | Explicitly relate the narrow current mode to the intended hosting goal; do not silently change either |
| Screen contract | Product/repository-oriented navigation and stage outcomes | Owner-selected current HTML shows a different operator console/navigation model | Name the selected visual authority and reconcile the conflicting screen directions before implementation |
| UI QA report | Passed checks and no actionable P0/P1/P2 issues for its reviewed surface | Current source probes and the selected prototype have different, untested behavior | Attach revision, artifact, scenario, viewport, and limitations to the claim |

Sources: [README](../../../README.md#L3), [current-status section](../../../README.md#L707), [planning index](../../README.md#L26), [active index](../../active/README.md), [active Repo Mode document](../../active/PHarness-Repo-Mode-V1-Product-and-Control-Plane.md), [screen contract](../../design/repo-mode-v1-screen-contract.md), [UI QA](../../../ui/design-qa.md).

The stable release can legitimately lag this branch. That is not itself documentation drift. The problem is presenting statements about the stable release, unfinished branch, and target design without consistently saying which one is being described.

## Promote the best existing introduction

The `living-docs` material already does much of the introductory work the README needs. It should be considered as an operator entry point after its claims and ownership are reconciled. Its current untracked state and dated snapshot should be visible; it should not silently become an authoritative substitute for source and release evidence.

A short opening could communicate the product without making the reader learn its internal resource graph:

> PHarness coordinates software changes through bounded coding work, review, and recorded delivery evidence. This checkout includes a repository-focused mode that ends at observed manual source merge. Cluster execution and connected delivery capabilities also exist; their availability depends on configuration and validated capabilities. Fully autonomous delivery and verification of a hosted product remain the broader goal.

This is suggested copy for the reviewed checkout, not a statement that Repo Mode is deployed or that every path has passed acceptance.

Follow that explanation with links to the existing bounded starting path, capability status, operator handbook, and contributor architecture map. There is no need to build a new documentation website.

## Make the source-of-truth order useful in practice

The planning index already declares an evidence hierarchy. The missing piece is consistent use of that hierarchy on the documents most likely to be read first.

Each current entry point should make three facts easy to find:

1. **What it describes:** stable release, current branch behavior, intended design, or dated observation.
2. **What it is based on:** source/release revision and the applicable design artifact.
3. **What remains unproven:** fixture-only behavior, unavailable dependencies, unrun live checks, or pending acceptance.

A short manual status note is sufficient. Avoid adding a docs-generation framework or a new metadata system merely to keep a handful of entry points accurate.

The existing stable baseline is documented as `v3-operator-cockpit`, release commit `597edaf0bb32baf84a23142d61e4c28ac2788191`, compiled source `8c3e2a7985d142cd32b19d6ea6d89fee76d43abc`. The reviewed branch is later. Helm values also name the older compiled revision and default Repo Mode off. This review did not inspect the live cluster, so none of those repository facts is a claim about what is currently running.

## Reduce repetition without erasing history

The product vision, operating model, product contract, screen contract, onboarding contract, stage evidence contract, and active implementation document repeat parts of the same story. Repetition is sometimes useful, but it multiplies places where vocabulary and scope can drift.

**Recommendation:** let each existing document own one question. Link to the owner for detailed definitions rather than copying the same lifecycle and navigation list into every artifact. Keep historical plans and evaluations, but mark them as superseded where appropriate and point to their replacement.

Do not erase old failed evaluations or inconvenient review results. Their value is the ability to compare claims with the evidence available at the time. A dated negative result is not a current defect until it has been revalidated.

## Separate operator language from contributor language

An operator needs to know what is changing, what is happening, what requires a decision, and what that decision authorizes. A contributor needs to know which handler, resource, state version, and executor owns the behavior.

Those are both valid documentation needs. They become confusing when the operator introduction leads with names such as `SourceDeliveryIntent`, `StageExecution`, and `RepositoryBinding`, or when a contributor reads “completed” without knowing the exact terminal boundary.

Use the existing glossary/domain documents for precise definitions. In the first-use path, introduce WorkItem as a desired change and Run as one execution attempt. Explain additional resource names when they become relevant. Keep exact identifiers available in examples and reference material.

## Evidence language should be narrower and more durable

Replace unqualified “passed,” “complete,” “live,” and “validated” with the actual observation: what ran, on which revision, using which inputs, with which limitations. For screenshots, name the design/bundle and viewport. For coding evaluations, retain model, prompt, fixture version, baseline, and attempt counts. For deployments, distinguish image/source identity, infrastructure readiness, and product acceptance.

The architecture graph's current checker failure is a useful example: the old graph can remain a historical structural map, but its earlier acyclic result should not be repeated as a current machine-checked guarantee. The same rule applies to prior UI QA and old coding-loop reviews.

**My candid view:** PHarness does not need more documentation volume. It needs one trustworthy front door, fewer duplicated declarations, and clear dates and authority on the material that already exists.
