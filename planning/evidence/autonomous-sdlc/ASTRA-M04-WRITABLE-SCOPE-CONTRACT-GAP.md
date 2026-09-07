# ASTRA M04: A shared writable-scope contract gap

2026-09-07. Evaluated source: `81edd9e81d280b0eda866b719c86562b3ff2f02c`. Evaluation: `infeval_01a07e2e99b77521b39a0cfb7bf42f00`. Status: diagnostic completed and failed; no qualification. This is the next M04 implementation boundary, not a request to lower its gate.

## Objective findings

[The native result](ASTRA-M04-81EDD9E-ONBOARDING-RESULT.json) contains one `python-contract` attempt on the existing `onboarding-minimax-m3-v2@v1` policy. The actual evaluator image and source are recorded in [the Job receipt](ASTRA-M04-81EDD9E-EVALUATION-JOB.json). The preceding [target protocol verification](ASTRA-M04-81EDD9E-PROTOCOL-VERIFICATION.json) passed 30/30. The diagnostic received no qualification.

The new shared prompt (`repo-onboarding-v2@2026-09-07.1`) reached the model in its retained initial context. The proposal now separates current onboarding permissions from future development scope and explicitly excludes the onboarding-only .pharness paths. It proposes `src/`, `tests/`, and `README.md` as writable paths. The previous source-92 proposal instead copied the three .pharness configuration files into future development scope. The two failed results therefore share an error category but differ materially in their proposals.

The new attempt made one `submit_onboarding_proposal` call in one turn, took 23,998 ms inside the stage, and reported 4,554 input tokens and 3,681 output tokens. There was no correction, changed path, approval pause or context-budget failure. The result is `stage_scope_or_coverage: undeclared_onboarding_write_scope`; `infrastructure_valid` is false in the native report. That flag is retained as reported; here the detailed cause is contract scope, not evidence of a Kubernetes outage.

The source establishes an interface mismatch:

- `crates/pharness-runhost/src/prompt.rs` describes writable_paths as nonempty strings, without explaining exact-file versus recursive-subtree semantics.
- `RepositoryContract::validate_candidate` in `crates/pharness-core/src/project.rs` accepts directory-looking strings such as `src/`; its relative-path validation does not reject a trailing separator.
- `project_path_glob_matches` in `crates/pharness-runhost/src/lib.rs` treats only a `/**` suffix as recursive. Otherwise it compares the complete string. Thus `src/` does not authorize `src/app.py`; it is not interchangeable with `src/**`.
- The onboarding scorer in `crates/pharness-eval/src/stage_suites/onboarding.rs` rejects paths outside its declared exact set. It caught the unusable proposal, while the earlier candidate validation had accepted its shape.

Do not normalize `src/` into `src/**` silently: that grants descendant write authority that the submitted expression did not carry. Do not change the expected answers or the frozen coding thresholds to make this result green.

## Assessment

The first scope clarification improved the observed output, but one uncontrolled before/after observation does not prove general model reliability or causation. The more concrete engineering defect is an underdescribed, inconsistently validated executable contract. Asking another model to guess that contract would confound a capability comparison.

My judgment: keep the current gateway and bounded runtime. Fix the shared path contract and early feedback before another expensive comparison. This evidence does not justify a new model backend or a broad orchestration rewrite. The stray closing `</item>` text in several assumptions is also a quality blemish, though it was not the rejection cause.

## Next implementation and acceptance

1. State the supported writable-path grammar in the production typed tool description and shared stage guidance: exact repository-relative files and explicitly recursive subtree patterns. Keep examples generic and independent of fixture answers.
2. Make new onboarding submissions reject ambiguous/noncanonical directory forms early with actionable feedback. Use the production validator and executor semantics consistently. Preserve historical reads, existing bounded authority and immutable dependency locks. Never expand a submitted path during normalization.
3. Add deterministic checks for actual descendant coverage, exact-file authority, trailing separators, traversal, sibling-prefix escape, forbidden onboarding-only paths and early invalid-submission feedback. Keep the original failed submissions and their verdicts intact. Do not merely change the scorer's allowlist.
4. Record prompt/tool-contract hashes and deploy a complete immutable source revision. Reuse the working Mac builder while lucas-desktop is off. Image builds, runtime checks and the native bundle remain required; the accepted source-81 release is a compatible baseline.
5. After release observation, check protocol-verification freshness before evaluation launch. Its native lifetime is 15 minutes, so an old receipt cannot cover a new session. Renew the existing bounded 30-case check if required, record its identity, then run one fresh unchanged-policy `python-contract` diagnostic. Only then resume common-input controls and the connected coding loop.

M04E/F and the frozen 24-task qualification remain open. The Windows local-model endpoint is separately pending owner setup; no local target is activated or qualified by this release.
