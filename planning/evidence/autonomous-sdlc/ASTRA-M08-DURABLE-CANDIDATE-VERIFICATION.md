# ASTRA M08: Durable staging candidate verification

Status: implemented and locally validated; **not deployed or accepted as autonomous delivery**.
Source: `f01b0bdd5ad9cfc7e300a5923366e9f77f38e327`. Fresh main: `bdf979a9b8ea375533155803010489083242545e`.
Branch: `codex/astra-staging-runtime-verification`.
The baseline admission and native reader slices are integrated dependencies.

## Resulting behavior

The existing controller now continues from the first recorded staging GitOps receipt
to the actual candidate deployment and one fixed five-minute runtime window. It
retains baseline admission, GitOps commit and candidate verification as distinct
evidence. A passed staging result releases that operation's locks; it leaves the
WorkItem open and waiting for production approval and verification.

The candidate must use the admitted immutable image. Its expected GitOps revision
is the first accepted GitHub observation that proved the admitted commit was included
and the changed file still matched. Later callbacks cannot substitute another commit,
image, baseline or window. The authority also binds the application, saved policy,
tested source, build, original plan and original baseline-admission hashes.

## Durable observation and authority

1. Revalidate original source/build/policy evidence. Verify the native baseline at
   its historical admission time; an expired historical baseline is inspectable but
   cannot authorize another write. Its admission and identity hashes remain bound.
2. Save an identity-attempt marker before the bounded native deployment read.
   Observe the exact Argo revision, Deployment/ReplicaSet/Pod ownership, running
   digest and ready Service endpoints. Wait for rollout within the existing 240
   checks and original one-hour operation ceiling. No replacement authority is minted.
3. Once identity matches, persist one window on the next 30-second query boundary,
   30–60 seconds after the original identity attempt. The window must follow the
   recorded GitOps receipt and fit inside the original operation. A bounded future
   worker clock causes waiting, not an observation that predates its commit receipt.
4. Collect the finite functional probes after window start. After window end plus
   ten seconds for ingestion, obtain the closing identity and collect Mimir/Loki
   evidence and the correlated health-request trace. Frontend traces remain explicitly
   uninstrumented; browser initialization still requires M11's application change.
5. Persist the native result and recompute its assessment at capture time. Validate
   it against its original identity marker, window, and before/probe/after receipts.
   Repair a missing projection link after interruption before completing the staging
   operation. Repeated ticks and commit callbacks preserve the same result.

Pausing stops new writes; this already-authorized observation continues. Interrupted
functional/final collection becomes an explicit inconclusive result instead of
silently consuming another read allowance. Missing telemetry, missed/expired windows,
contradictory receipts or exhausted rollout observations block promotion. A historical
passed result remains historical evidence; it is not renewed into a fresh approval.
Changing bound source after a pass blocks further progression and preserves that result.

## Contracts and compatibility

No database migration, new backend, new scheduling service, model switch or budget
increase was added. The existing SQLite claims, WorkItem, deployment intent and
workflow operation carry the state. Immutable artifacts use the existing store.
The deployment has a separate `hosted_staging.runtime_observation`; operation refs
retain `staging_runtime_window` and `staging_runtime_result` beside the original
source/build/commit records. GET routes remain read-only.

The preceding baseline gate introduced staging authority v1alpha2. It and this
controller require a compatible worker/API release before new hosted writes are
enabled. The currently deployed `19b0c55` remains the schema-54 reader floor while
creation is disabled; it is not the minimum compatible active v1alpha2 writer.
Record the actual integrated release as the recovery floor before enabling that flow.

## Validation

**299 API/admin tests passed**, including **22 staging checks**. The eight candidate
cases cover old versus exact image/Argo identity, observation while paused, missing
telemetry, recovery of an incomplete result projection, duplicate callbacks and
repeated ticks, historical result validation, changed source after a pass, mixed
native receipts, a future worker clock, a window before the commit, interrupted final
collection and exhaustion of the original identity allowance. Multiple assertions
belong to a single case; these are not additional independently counted tests.

All-target API Clippy with warnings denied, formatting, module boundaries and five
dependency-parser checks passed. [Validation and source/log hashes](ASTRA-M08-DURABLE-CANDIDATE-VALIDATION.json)
record the tested implementation. Earlier passing focused runs are retained. These
SQLite/native-reader fixtures are synthetic. They do not count as the real staging
deployment required by M08 or either autonomous application change required by M11.

## Deployment and recovery

Review and merge the integrated native readers, finite namespace access, baseline
gate and candidate controller. Build one merged source revision with the current
seven-image/native-bundle release procedure; pin and observe exact live identities.
Do not change the running qualification environment partway through an evaluation.
Keep hosted creation disabled until its exact-runtime qualification and prerequisites
pass. Then demonstrate the real authorized source/build/GitOps/Argo/runtime chain.

No production action is implemented or authorized by this slice. Candidate failures
remain failed or inconclusive evidence and do not automatically restore or promote
an image. M09 must implement the separate production approval and safe recovery
authority. Its approval must freshly revalidate deployment/evidence before merge.

The controller currently requires the recorded GitOps revision exactly. An unrelated
external GitOps change during the window can block verification; it cannot be silently
adopted as equivalent evidence. This is an explicit limitation, not a reason to make
missing or changed evidence green. Frontend configuration behavior and a genuine
autonomous yfinance release remain open acceptance gates.
