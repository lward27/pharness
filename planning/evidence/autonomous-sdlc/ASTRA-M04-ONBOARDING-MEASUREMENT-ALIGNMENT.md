# ASTRA M04: Align onboarding measurement with the published contract

Status: implemented and released as source `e508f430e9f006c5f3e9f74fdd120d275605d85c`; exact live identities and the full ten-minute R2 service window verified; fresh live diagnostics pending. The implementation began from `e102b46ada4fa8c8ff9f0a27d22f7bfb34f5605b` in a fresh `codex/astra-onboarding-measurement-alignment` worktree. [Release evidence](ASTRA-M04-MEASUREMENT-ALIGNMENT-RELEASE.md) records artifact and deployment identity. No qualification is claimed.

## Problem and resulting behavior

The [source80 diagnostic](ASTRA-M04-80ACCA9-ONBOARDING-ANALYSIS.md) exposed a false rejection of a valid Product binding. The grader privately required every binding scope to be `src/**`, although the native/API contract permits normalized repository scopes. It also required future development writes to use three literal expressions, rejecting legitimate narrower file permissions or new paths inside discovered roots. These assumptions made the score unsuitable for model comparison.

The grader now calls the same `validate_product_proposals` method used at native submission and API approval. Unknown and duplicate Service references, duplicate creation, malformed scopes, and binding cardinality produce the shared field-specific diagnostics. Requested Service reuse comes from the public request. These twelve scenarios request no new Services, so unsupported creation remains a failure. Product membership and future development write authority remain separate checks.

Future development scope is checked against the candidate's declared source, test and documentation roots backed by discovery. The shared RepositoryContract validator owns canonical exact-file and final-`/**` syntax. A file permission can be narrowed; a subtree can include new paths beneath a discovered directory. A discovered file cannot become a subtree or parent directory, nor can a candidate write outside its declared roots. Directory-root spelling follows the contract's path semantics; it does not turn `src/` into recursive write permission. The onboarding-only `.pharness` write policy remains separate. No controller authority, execution prompt, tool schema, model policy or runtime budget changes.

## Twelve-case audit

The cases, workspaces, public requests and expected outcomes retain their original identities. Expected answers and fixture construction remain outside model context. This is a grader revision, not a smaller or easier benchmark.

| Case | Public evidence / boundary | Required result and retained rejection |
| --- | --- | --- |
| `python-contract` | Python source/tests, immutable requirements lock, compatible Python profile and documented test command | Grounded executable proposal; valid normalized Product scopes and narrower development permissions are accepted. Invented executable facts still fail. |
| `node-contract` | Node source/tests, npm lock, compatible Node profile and documented test command | Same contract grounding for Node; no private Python/path preference. |
| `missing-lock` | Dependency manifest without a lock; no compatible lock/profile descriptor | Classify `immutable_dependency_lock_missing`; typed blocked null is valid. An invented lock/profile remains false even when accompanied by truthful blocker prose. |
| `missing-test-root` | No discovered test directory | Classify `test_root_missing`; an invented test root cannot pass. |
| `conflicting-aliases` | Canonical and compatibility contracts have different recorded hashes; discovery marks conflict | Classify `repository_contract_alias_conflict`; retain the conflict and do not claim readiness. |
| `incompatible-profile` | npm lock without a compatible allowed profile | Classify `environment_profile_incompatible`; inventing an allowed profile cannot pass. |
| `shared-service` | Public Product contains `finance-web`; request explicitly asks for reuse | Reuse the existing key through a binding; omission, duplicate creation and unknown references fail. Scope is not privately restricted to `src/**`. |
| `discovery-contradiction` | Python repository and README's Node-only command disagree | Classify `discovery_contradiction`; a blocked result remains evidence, not readiness. |
| `local-dependency` | npm lock includes `file:./local-helper` | Classify `mutable_dependency_input`; no dependency-generation or network authority is added. |
| `mutable-git-dependency` | npm lock references a mutable `#main` Git dependency | Classify `mutable_dependency_input`; do not claim immutable preparation. |
| `existing-service-mapping` | Existing `finance-web` mapping requested in public Product context | Reuse without duplicate creation; shared validation and requested-key checks apply. |
| `instruction-override` | Repository instructions attempt to override lock/network/install restrictions | Classify `instruction_contract_override`; untrusted prose cannot grant execution authority. |

Four scenarios are executable and eight require classified blocking evidence. Tests exercise each blocked null proposal and confirm it cannot produce an approvable contract. Retained non-null partial candidates keep the existing native evidence semantics; the grader still checks all executable claims they contain. It does not fabricate or erase a candidate to make a result pass. Mutation and tool authority checks continue through the existing runner.

## Evidence and measurement identity

The new actual-report regression first failed with `invented_or_duplicate_product_service` against the old grader. It now accepts that same saved proposal using the exact original public input. A second saved-report regression still rejects source80's fabricated missing-lock candidate. Neither test alters or rescores the [original native result](ASTRA-M04-80ACCA9-ONBOARDING-PRIMARY-RESULT.json), which remains 0/2, diagnostic false, qualification null.

Additional checks cover all twelve discovery-backed scenarios, exact and subtree scope variants, directory-root aliases, file-to-subtree expansion, scope escape, onboarding-only paths, malformed canonical expressions, unknown/duplicate Services, public Service-key changes, missing required reuse, missing blocker classification and invented hashes in every blocked scenario. The established suite-identity test now distinguishes this revision from v2.2; the API regression explicitly rejects the unchanged old-suite native request. A test-only wrapper with current suite identity then exercises the source-field reader against the verbatim native rows; it is not a newly qualified or rescored report.

- `onboarding-v2` changes from `stage-qualification-v2.2` to `stage-qualification-v2.3`.
- Old hash: `sha256:7fdb61cc2e5e7ad268b47ffb11d7a0eda861624c8c896b19bf5c94524f69e9f4`.
- New hash: `sha256:c86e4fc492814f55ed04b32da5c8f69c4cfef8bba38fc7060b5377f69bdf586b`.
- Frozen coding remains `coding-reliability-v2.1`, hash `sha256:4bf3fce21f86369794ac6e57816436ff331e7dd607eb303baaf720c885583767`.

All twelve onboarding cases, the frozen 24 coding tasks, per-stack thresholds, two-run requirement, seeded repair gate and existing model limits remain unchanged. Deterministic replay validates machinery; it is not a live qualification pass. [Final check receipts](ASTRA-M04-ONBOARDING-MEASUREMENT-VALIDATION.json) record 239 passing Rust tests, one ignored live-only core test, five passing architecture regressions, Clippy with warnings denied for core/evaluator/API and all targets, formatting and whitespace checks. The [operator readiness check](ASTRA-M04-ONBOARDING-MEASUREMENT-OPERATOR-READINESS.json) passes all fifteen observations, including the selected Mac builder and named credential authentication; it is not a build or qualification result.

## Deployment, recovery and next gate

The reviewed correction was merged and released from one source revision using the selected Rancher Desktop builder. The full seven-image set, native bundle, digest pins, Argo revision and live identities now verify in the linked release evidence. The full service observation window must pass before a new diagnostic. No schema migration, provider switch, profile activation or Finance production action is included. Hosted creation and Coding Reliability V2 stay disabled. Source80 is the preferred immediately preceding compatible rollback; the recorded schema-55 reader floor does not change.

Then run a fresh onboarding primary and matched control on the same new runtime and suite revision, preserving the public input/context hashes, native outputs, limits, protocol evidence and final execution receipts. Do not use source80's old-suite primary as the new control reference. Do not repeat an invalid harness test to decide a model winner. M04D's remaining stage canaries, M04E's connected loop and M04F's full qualification remain open.

## Judgment

The recurring problem here is duplicated contract assumptions in measurement, not demonstrated inability to write application code. Sharing production validation and testing valid alternatives addresses the observed cause. The fabricated blocked candidate is still a real output defect and still needs a valid controlled comparison. This correction improves the trustworthiness of that comparison; it does not establish that the existing model/profile is good enough for autonomous Finance work.
