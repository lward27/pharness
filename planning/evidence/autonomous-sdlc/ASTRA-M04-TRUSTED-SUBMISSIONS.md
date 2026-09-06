# ASTRA M04A: Controller-bound onboarding submissions

Date: 2026-09-06. Source: `8faf0fb9a0a2e679aee6cc83ed2b94373189071d`. Base: `ddcc4b102ffe4651ca70e5f0b88e5d8f951dc5b7`.
Status: implemented and locally validated; not deployed or live-qualified.
Authority: the owner-approved [M04 refactor](../../programs/autonomous-sdlc/ASTRA-04-CODING-RELIABILITY-QUALIFICATION.md).

## Result

The onboarding model submits proposed configuration, instructions, services/bindings,
assumptions, conflicts, blockers and readiness claims. The native runner attaches
`schema_version`, `discovery_id` and `discovery_hash` from the original Run. An
accepted proposal still reaches the existing controller and database as a complete,
identity-bound RepositoryOnboardingProposal. A blocked proposal remains blocked;
normal validation and later approval/source checks are unchanged.

The saved Run records `pharness.dev/onboarding-agent-submission/v1`. The API chooses
it from the original pinned stage-prompt selection, rather than a newly compiled
profile or the submitted payload. The worker checks its exact constrained tool hash
before model execution. Missing or conflicting original context fails before dispatch.
Unknown contract versions fail closed.

New-contract submissions containing a controller-owned identity field are rejected,
including when the field happens to match. Legacy submissions still require their
original full identity and reject ID/hash disagreements with field-specific errors.
Missing semantic fields and unknown fields also receive precise native rejections.
The adapter never silently overwrites an identifier supplied by the model.

## Compatibility and measurement

Saved Runs without the new marker keep the legacy tool and onboarding stage prompt.
Historical V1 and V2 durable proposal formats remain readable; there is no migration.
Serialize/reconstruct tests preserve the original contract and discovery, even if a
separate newer Run uses a different discovery. Full checkpoint and approval/budget
resume testing is M04B; a serialization check does not close that gate.

The Onboarding prompt advances to `2026-09-06.2`; its qualification suite advances
from `stage-qualification-v2.1` to `stage-qualification-v2.2`. All twelve scenario IDs,
workspace/discovery construction and semantic scoring predicates remain unchanged.
The model-side replay now omits controller identity; the complete shared-runner replay
must produce and score the fully bound native ToolFinished document. The raw
[validation record](ASTRA-M04-TRUSTED-SUBMISSIONS-VALIDATION.json) maps every case one-to-one.
No historical outcome is rescored. Frozen coding/repair tasks and thresholds, model
registry, profile budgets and exact-runtime activation logic are unchanged.

## Objective validation

**530 distinct component tests pass** across API/admin, core, evaluator and runner;
one existing live-only core test remains ignored. The initial combined run found
one assertion still expecting the previous Onboarding suite revision. That assertion
was updated to the approved explicit revision and passed its targeted rerun. All
other evaluator checks passed in the initial run. Both the initial failure and the
successful correction are retained, rather than described as one clean run.

Coverage includes ready/blocked native submissions, missing/unknown fields, conflicting
and malformed discovery, legacy formats, saved-selection compatibility, bounded
candidate validation, all twelve controller-bound Onboarding replays, and frozen
coding public/hidden replay checks. These are deterministic fixtures, not model scores.
Four-package all-target Clippy, formatting and the five architecture checks pass.
Clippy was repeated for the evaluator after the revision assertion changed.

## Deployment and recovery

No deployment, provider call, Finance source change or production action was made
for this slice. Deploy API and worker/evaluator artifacts from a common immutable
source before creating new-contract Runs. Keep hosted creation and Coding Reliability
V2 disabled until the complete exact-runtime qualification succeeds. The first release
containing this adapter becomes the executable compatibility floor for active
new-contract Runs; older readers can read the full stored proposal but cannot execute
the new tool contract. Do not treat reader compatibility as permission to downgrade
active workers. The program's existing database/history preservation procedure applies.

## Design judgment and next step

This removes avoidable copying without moving judgment into another agent. It does
not establish that the chosen model will reason correctly about a repository. M04B
must make context delivery inspectable through initial and resumed requests; M04C
must repair the stage measurements before small live canaries and the connected loop.
No full matrix rerun is justified by this local success alone.
