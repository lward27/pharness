# ASTRA M04: Correct diagnostic completeness and Planner acceptance inputs

Status: implemented and locally validated; not released or qualified. Base source `040caeb6ae1a47997583204510bef08bc9f5766b`; live diagnostic source remains `e508f430e9f006c5f3e9f74fdd120d275605d85c`.

## Behavior and scope

A complete two-case diagnostic previously inherited the full twelve-case stage gate's `infrastructure_valid: false`. The report now checks the exact requested case set, one attempt, unique rows and absence of an infrastructure abort. A completed model failure remains a valid measurement with `diagnostic.passed: false`. Empty, partial, duplicated, wrong-case, wrong-attempt and aborted diagnostics cannot claim success. The nested full-stage gate is unchanged, and diagnostics always keep `gate_passed` and `candidate_safe` false.

The API independently checks native rows and abort evidence rather than trusting the producer's completeness flag. Historical complete reports with the old false flag remain readable. Original diagnostic JSON, scores and qualification records are not rewritten; no migration or model-policy change is needed.

The [retained Planner result](ASTRA-M04-E508F43-PLAN-PRIMARY-RESULT.json) exposes a separate input contradiction. Eight of twelve requests require `unit` and `compile` but select one. The worker and grader correctly reject the unselected name. Correct only those eight `spec.acceptance` lists to select both; preserve every scenario, source file, baseline, path restriction, prompt and model setting. Planner alone becomes `stage-qualification-v2.4`, hash `sha256:6379bdb4ac0d12ef87e0074154aaf20400cbd250cca8f402984de7a893190467`. All other suite revisions remain unchanged, including frozen coding `coding-reliability-v2.1` and seeded Repair `repair-reliability-v2.1`.

The twelve Planner case identities are unchanged. The eight corrected selections are cross-repository-context, failing-baseline, ambiguous-intent, immutable-source, undeclared-path, documentation-boundary, stale-context-revision and path-intersection. The other four already selected both commands. The original suite remains historical evidence; its diagnostic cannot serve as a new-suite control reference.

## Validation

[Validation receipt](ASTRA-M04-MEASUREMENT-CONTRACT-CORRECTIONS-VALIDATION.json): **523 Rust tests passed**, one existing live-only test ignored; scoped Clippy with warnings denied, formatting, and five architecture regressions pass. Both new defect regressions failed against the old behavior before the corrections. Tests exercise complete and incomplete diagnostics, aborts, historical reader compatibility, the full twelve-case public acceptance contract, and the real retained Planner submission through both the evaluator and production worker validators. Existing full-stage, frozen coding, seeded repair, hosted authority and exact-source tests remain in the passing run.

A structural test accepting the retained plan with the corrected selection is not a rescore or a semantic endorsement. Its proposal to fix an unrelated pre-existing `/legacy` failure still requires connected-loop scrutiny. Neither local checks nor the diagnostic report field establishes autonomous coding reliability.

## Deployment and recovery

Finish or reconcile every sourcee508 diagnostic operation before releasing this correction. Build the required immutable artifact set from one merged revision, pin all components, and verify Argo plus live identities and the complete observation window. No Finance source or production GitOps operation is authorized by this release. Keep hosted creation, Coding Reliability V2 activation, model defaults and execution limits unchanged.

Schema remains 55. Preserve the source-92-compatible reader floor; use the preceding compatible complete release for rollback if needed. Old reports remain truthful historical records. A new Planner primary/control pair must use the new suite and same deployed runtime; sourcee508's invalid Planner control remains undispatched. M04D, connected-loop M04E and full qualification M04F stay open.
