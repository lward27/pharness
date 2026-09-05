# ASTRA M04: Truthful onboarding proposals and qualification fixtures

Status: implemented and locally validated. Not deployed or live-qualified.
Source baseline: `5b0d815c9a70521d843f23d78cf38c383c312ab9` (current main verified 2026-09-05).
Branch: `codex/astra-onboarding-contract-readiness`.
Live PHarness remains `48c77b7b4438d621ff9563b913857bcf771f1800` while its serial qualification Jobs run.

## What was wrong

1. `submit_onboarding_proposal` required a complete RepositoryContract even when
   discovery lacked the facts needed to construct one. An agent could describe a
   missing prerequisite but could not submit an explicitly absent candidate.
2. Proposal persistence marked every accepted revision ready and cleared the
   onboarding blocker list. Approval did not explicitly reject the proposal's
   own blockers or conflicts. Historical records need the same guard as new ones.
3. Every Onboarding V2 workspace was the same Python directory, including the
   Node, absent-root, missing-lock, conflicting-alias, and mutable-dependency cases.
   Its fixed candidate hashes did not describe the actual lock bytes. Most facts
   were scenario prose; the proposer had no `get_evidence` tool for the separate
   candidate-shaped fixture evidence.
4. The scorer searched the whole submitted JSON for a blocker code and checked
   a profile name. It did not establish that a proposed lock, root, command, or
   reused Service actually existed. A warning in instructions could conceal an
   unblocked fabricated contract.
5. The console turned a null candidate into an empty object when editing. It
   also displayed “No blockers” for an absent readiness assessment, even though
   no such assessment existed.

These are source-proven defects. The old 48c77b7 Onboarding result remains
**0/12 in both attempts**, with the original report unchanged. It lacks the exact
rejected arguments, so these defects do not establish the cause of every old
failure. See [the failed run](ASTRA-M04-48C77B7-ONBOARDING-FAILURE.md).

## Implementation boundaries

- New proposal schema: `pharness.dev/repository-onboarding-proposal/v1alpha2`.
  `candidate_contract` can be null only with explicit nonempty blockers or
  conflicts. Complete candidates still undergo RepositoryContract validation.
  Shared validation enforces exact discovery hash format, bounded statements,
  bounded instructions, a forecast object, and the existing 128 KiB submission cap.
- Storage retains immutable proposal revisions and projects unresolved statements
  as `proposal_blocked`. It independently refuses approval of blocked, conflicted,
  absent-candidate, or malformed blocker records before product-model writes.
- The API retains blocked drafts against their exact successful discovery, but
  cannot approve or materialize them. Complete, unblocked proposals still require
  compatible active profiles, discovered locks and roots, and valid topology
  proposals. Approval, patch contexts/outcomes, and merged-contract validation
  reject unresolved historical proposals as well.
- Blocked onboarding appears in the overview's attention list. Read-only visits
  preserve its version and perform no retries. An operator can correct a proposal
  against the same evidence or register changed source and create fresh onboarding.
  Source-delivery intents continue to prevent edits to already-dispatched proposals.
- Lamina preserves the absent contract when editing and states it plainly. Missing
  readiness is no longer represented by empty passing-looking facts. Forecasts
  are explicitly unverified. Existing layout and navigation remain intact.

No SQL migration, new backend, budget increase, model switch, Finance application
patch, production approval, or live rollout is part of this slice.

## Qualification correction

The same twelve Onboarding V2 scenario identifiers remain. They now contain
actual Python or Node files, real command declarations and hashes, absent test
roots/locks where specified, two conflicting contract aliases, mutable local/Git
dependency specifications, contradictory documentation, or untrusted instructions.
The existing production discovery reader inspects each repository. Its inventory
hash is verified, and deterministic fixture commits are checked again when each
execution workspace is created. The expected contract is never sent as controller
context or as fixture evidence, including on the deferred native-host path.

The revised scorer checks discovery identity, blocker placement, exact compatible
profile, actual dependency path/kind/hash, existing roots, discovered commands,
bounded write scopes, and reuse of the existing Service. Independent negative
checks reject invented facts and duplicate Services. A blocked response counts
only as correct handling of that fixture, not as application or delivery success.

Some missing-profile/lock cases deliberately exercise a blocked tool submission;
production readiness can reject these before launching a model when there are no
compatible profile descriptors. This is a contract test, not evidence that a live
onboarding traversed those stages. Empty dependency locks in the positive Python
fixture describe an application with no external runtime dependencies.

The affected suite advances from `stage-qualification-v2.0` to
`stage-qualification-v2.1`; its hash must differ. The Onboarding stage prompt alone
advances to `2026-09-05.2` and its tool schema advertises the nullable candidate.
Builder/Planner/Repair/Test Diagnosis/Verifier prompt content, configured targets,
profile budgets, frozen 24-task coding/Repair inputs, and numeric gates remain
unchanged. The base prompt bundle stays `2026-09-05.1`; the Onboarding stage prompt
and tool hashes distinguish this correction. Fresh protocol and live stage
qualification are mandatory on the eventual immutable runtime.

## Validation and evidence

[Validation record](ASTRA-M04-ONBOARDING-CORRECTION-VALIDATION.json) records 554 distinct
Rust tests validated across full and focused runs, 95 UI tests, a successful UI
build, four browser cases, passing Clippy and architecture checks, source hashes,
unchanged frozen inputs, and logs. The evaluator's 29 passing cases plus its
corrected revision assertion comprise 30 distinct passing tests; this was not one
uninterrupted all-green run. The final API round-trip and shared-core boundaries
were rechecked. The record also preserves the resolved interim failures.

[Local replay](ASTRA-M04-ONBOARDING-V21-REPLAY.json) passes 12/12, including truthful
blocked outcomes. Its runtime label is the package version `0.1.0`, not an immutable
release SHA, and its provider is deterministic replay. It cannot close the live
model qualification gate. The revised suite hash is
`sha256:1ecbebcb61abc30ede919fbd248f56215066313f640cf18baed1b57f7b118d1e`;
the revised Onboarding tool hash is
`sha256:e2590910fbeb270b4dd9fa41ca862c97efda05f82e5ccd471b6c2ae5c3c5a636`.
The exact stage prompt content hash is recorded separately in the validation file.

Browser evidence uses explicit presentation fixtures. It proves neither discovery
accuracy nor a real Finance deployment. The new blocked view is exercised at desktop
and phone widths, in both themes, with keyboard editing and no navigation writes.
The existing proposal-approval view and state-bound confirmation are also checked.

- [Desktop, dark](ASTRA-M04-ONBOARDING-BLOCKED-DESKTOP-DARK.png)
- [Desktop, light](ASTRA-M04-ONBOARDING-BLOCKED-DESKTOP-LIGHT.png)
- [Phone, dark](ASTRA-M04-ONBOARDING-BLOCKED-MOBILE-DARK.png)
- [Phone, light](ASTRA-M04-ONBOARDING-BLOCKED-MOBILE-LIGHT.png)

Subjective result: missing facts and the blocked condition are clearer. The page
still has long technical identifiers, dense JSON editing, and independently scrolling
panels. This is an integrity correction with a small usability improvement; it does
not close M10 or substitute for the owner's visual review.

## Deployment and recovery

1. Finish the running 48c77b7 evaluations without restarting their API or changing
   their inputs. Retain every failed result and dispatch Repair only after the
   preceding serial evaluations are terminal.
2. Merge and release the correction with the other pending source/build/runtime
   slices using the immutable seven-image/native-bundle procedure. Pin the merged
   source and digests, verify Argo's observed revision and live image identities,
   and deploy compatible API, workers and UI before V2 proposal writes.
3. Keep hosted creation and Coding Reliability V2 disabled until the exact deployed
   revision satisfies every qualification gate. Run fresh Onboarding protocol and
   both semantic attempts with the recorded hashes. Do not rescore the old report.
4. Continue reading complete v1alpha1 proposals; do not rewrite historical evidence.
   Null candidates require v1alpha2. Blocked or incompatible historical records may
   require correction instead of the previously offered approval/materialization.
5. Schema 0053 remains unchanged, but schema compatibility alone is insufficient:
   after a V2 proposal is stored, a release predating this correction is not a safe
   rollback reader. Record the first merged compatible release and keep it as the
   minimum rollback version. An earlier release may misclassify the blocked record.

M04 remains open. M11 still requires real PHarness-generated Finance changes and
genuine human production approvals; none is supplied by these fixtures.
