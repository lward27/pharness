# ASTRA M04C/M06: Preserve the Planner decision boundary

Status: implemented and locally validated. Not deployed or qualified.

Base: `ce98a26065cd48323b58b8905fcbbfc6c76452cd`, whose compiled runtime is `d675159cb205d222616d253d311706be1889bb91`. The current runtime remains unchanged during its declared diagnostic sequence.

## Problem and resulting behavior

The d675159 Planner primary and same-model control both returned native2/2 on valid matched inputs. Both identified an unresolved `/legacy` baseline conflict; the control recommended changing the existing expected200 assertion to the observed404 behavior and deferred the choice to implementation. The old Planner seal retained that text only as an agent claim while recording empty contradictions/risks. Automatic approval therefore lacked a reliable way to stop on a stated unresolved decision.

New Planner Runs save `pharness.dev/planner-agent-submission/v1`, selected from the exact saved inference prompt revision. Their tool schema requires `readiness.status` (`ready` or `needs_decision`) and bounded `readiness.blockers`. Ready with blockers, needs_decision without blockers, unknown fields/status, missing new-contract readiness and malformed statements are rejected. The typed readiness claim does not assert that the plan is objectively correct.

A structurally valid decision-required plan remains a proposed WorkPlan, while its stage and WorkItem become blocked. Blockers survive as agent-origin contradictions and residual risks remain visible. The original full submission remains in agent claims. Hosted automatic plan approval, implementation authorization and ChangeSet approval require an explicitly ready plan in addition to the existing exact-revision/evidence checks. Marking a plan approved does not bypass the check before automatic implementation.

Existing source-only submissions and saved older tool schemas/prompts remain readable; absence of readiness does not silently select the new contract. Historical plans without readiness cannot acquire new hosted automatic approval. There is no SQL migration, new worker/backend, provider/default change, increased limit, production action or automatic incident campaign.

## Measurement contract

Planner's stage prompt advances from `2026-09-05.1` to `2026-09-08.1`; Planner qualification advances from v2.5 to v2.6. All twelve case IDs, source fixtures, commands, requirements and selected acceptance names remain unchanged. Each private expected record now includes a readiness disposition. The unrelated failing-baseline case requires a decision. The other eleven have sufficient authority to proceed, including ambiguous-intent, which explicitly instructs preservation of the existing representation.

The new grader checks this typed disposition and its structural consistency, not English keywords, negation or free-form semantic claims. Expected dispositions remain evaluator-private; no answer marker is added to model-visible inputs. Native v2.5 scores and historical reports are not edited or rescored. Frozen coding and seeded-repair tasks remain unchanged. This finite check is useful but insufficient: a model can still falsely claim readiness on an unmeasured case.

## Validation and limitations

[Component validation](ASTRA-M04-PLANNER-DECISION-VALIDATION.json) passes 590 distinct tests: 300 API/admin, 177 core/integration, 50 runhost and 63 evaluator checks; one existing core test is ignored. The evaluator full run passed62/63, then the stale revision assertion was corrected and its focused recheck passed. The final runhost check includes real context assembly with validated saved inference bindings. Clippy with warnings denied, formatting and all five architecture checks pass. The final API run passes298 tests after shared exact-plan validation; the added stale-plan regression also passes. A post-verification plan edit cannot pass ChangeSet approval merely by retaining a ready claim. Logs, source-file hashes and initial test failures are retained. Seeded negative child tests intentionally fail and their parents check that failure. No local test or replay counts as an autonomous Finance change.

Required checks cover valid and malformed submissions, legacy schema/prompt selection after serialization, exact current tool hashes, saved prompt selection despite changed defaults, real Planner sealing with retained blockers/risks, repeated controller reconciliation without a new Run, automatic approval and implementation admission, and correct ready/needs_decision grading. Existing source, build, staging, evidence and replay tests remain required.

The first complete API test run found old hosted fixtures without readiness and a test that incorrectly expected a proposal after an invalid submission. The fixtures now explicitly represent ready hosted plans while source-only fixtures preserve the historical shape; invalid submissions may retain their prior draft but cannot be approved. Production guards were not relaxed to accommodate those fixtures.

## Deployment and recovery

Finish and retain the already-dispatched d675159 Verifier/Diagnosis operations before changing the runtime. Merge validated implementation, integrate the separately validated evaluator layer-reuse preparation if appropriate, then freeze one source and use the established full immutable release procedure. Build and verify all seven images and the native bundle, pin digests, observe Argo and live identities, then pass the full service window.

Keep hosted creation and Coding Reliability V2 disabled until the exact new runtime qualifies. Recheck the exact Planner policy protocol and the affected ready/failing-baseline canaries on v2.6, with a matched control and complete inputs. M04E must demonstrate that an unresolved plan stops without a generated application patch and that an authorized ready plan advances through coding, deterministic Test, one correction and independent verification. M04F retains both full frozen qualifying runs and every existing threshold.

No reader migration is required. The prior runtime remains useful as a service rollback while hosted activation is disabled, but it does not enforce this new execution boundary. Suspend autonomous development/approval before rollback; do not let older code start or reinterpret new-contract Planner work. Preserve original Run/effect records and reconcile them by identity. Finance production approval remains a separate human action.

## Judgment

This repairs a concrete authority gap using the existing plan, stage outcome and approval machinery. It does not solve arbitrary model judgment, and it must not be presented as proof of safe autonomous planning. The connected-loop gate remains the place to prove preservation of regressions and truthful failure handling. F02 now includes Planner caveats; F13 remains open.
