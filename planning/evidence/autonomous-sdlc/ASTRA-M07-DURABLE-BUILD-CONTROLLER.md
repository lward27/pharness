# ASTRA M07: Durable hosted build progression

Status: implemented and validated locally; deployment and autonomous acceptance
remain open. Initial source base: `252030cdd2e457e4658ed7489c7e6a833add2f28`.
Refreshed against documentation-only main `e4948646a6f25a92fc1382d86a633c6d46a9bf30`
on 2026-09-05. This record is committed with the implementation.

## Behavior

The existing hosted controller now progresses a sealed source merge into one
finite Finance build. It reuses the WorkItem, PipelineIntent, permission grant,
workflow operation, repository lock, Release stage and existing isolated Tekton
executor. No additional controller deployment, coding provider or SQL migration
is introduced.

Before dispatch it persists the original PipelineRun, execution and operation
identities, source and policy hashes, grant, worker images, complete executor and
observer Job manifests, and deadline. Reconciliation may recover a missing
executor before admission, using those same records. Pause prevents preparation,
executor recovery and new PipelineRun admission. Resume does not renew authority.

Immediately before PipelineRun creation, the worker requests one durable
admission. The API revalidates current source, the finite contract, saved policy,
control state, original grant and exact manifest. Admission is recorded before
the external create and is not repeatable. An uncertain admission response does
not authorize a create. An uncertain Kubernetes response leads to observation
of the original run. Actual Kubernetes UID and requested fields must agree;
a replacement run is rejected.

After admission, a lost or finished executor can start one separately recorded
recovery observer. It uses the existing read-only `pharness-worker` Kubernetes
identity and cannot create PipelineRuns. Its dispatch is fenced before the
external call, so a disappeared Job cannot acquire a new observation budget.
This read-only recovery continues during pause. If that one observer cannot be
recovered or cannot deliver terminal evidence, the work remains blocked for
intervention. A late valid outcome can still be recorded.

The build window retains the existing one-hour controller bound. The grant uses
the original operation time and existing thirty-minute authorization ceiling;
expiry or revocation cannot issue a replacement grant. Job deadlines and
retention settings are preserved. Build failure, missing declared source/image
results, changed source authority, or conflicting evidence stop delivery.
The legacy manual retry route cannot allocate a new hosted build.

## Evidence and completion semantics

New worker-authenticated endpoints admit an execution and accept its observation:

- `POST /api/internal/pipeline-intents/:pipeline_intent_id/execution-attempt`
- `POST /api/internal/pipeline-intents/:pipeline_intent_id/hosted-execution-outcome`

Receipts bind the original manifest hash and PipelineRun UID. Submitted callbacks
cannot rewind a terminal outcome. Identical receipts are repeat-safe; conflicting
receipts are rejected. Reconciliation recovers a stored receipt before proceeding,
including an interruption between receipt creation and intent update. The older
callback cannot bypass these checks for hosted work.

Terminal analysis must agree with the actual declared `SOURCE_COMMIT`, `IMAGE_URL`
and `IMAGE_DIGEST`. The prior handoff's exact-source and finite-image checks remain
in force. A successful build records its own verified result and leaves Release
unsealed and the WorkItem open. Staging, human production approval, deployment and
runtime verification retain their separate acceptance boundaries. A failed build
does not become a successful WorkItem merely because its outcome was reconciled.

## Validation

The complete offline workspace suite passed **672 tests**, including 274 API tests,
149 core tests, 37 worker unit tests and a compiled-worker integration test with
four transport scenarios. The integration test uses a local HTTP server, fake
Kubernetes command and cleared process environment; it cannot contact the cluster
or inherit credentials. It covers denied and lost admission responses, a lost
Kubernetes create response, and recovery that performs reads only.

Controller cases cover duplicate and concurrent admission, pause/resume, missing
executor recovery, missing observer without redispatch, exact retained identities,
expired original preparation, contract retirement, unadmitted and conflicting
callbacks, duplicate failed callbacks, rejected manual retries, missing declared
source results, and an open WorkItem after build success. Route characterization
passes with 230 routes. Clippy on API/core/worker with warnings denied, formatting,
diff checks and architecture checks including five parser cases passed.

A review tightened the observer's Kubernetes identity and added its durable
dispatch fence. The first assertion used the wrong test-fixture name for the
existing executor ServiceAccount; it was corrected against that fixture before
the full passing run. Earlier focused runs and the final source/log hashes are
retained in the [validation record](ASTRA-M07-DURABLE-BUILD-CONTROLLER-VALIDATION.json).
Live authorization checking also confirmed that `pharness-worker` cannot create
PipelineRuns in `tekton-pipelines`.

## Deployment, recovery and remaining gates

This implementation has dispatched **zero live hosted builds** and created no
application patch, PipelineRun, deployment or production approval. The earlier
real Finance builds establish the pipeline boundary; they do not establish this
autonomous controller boundary or M11 acceptance.

Keep hosted creation disabled. Complete matching-runtime qualification, the
source credential prerequisite and the owner branch-protection decision. Then
release all required images and the native bundle from one merged source revision,
verify the live identities, and exercise this controller through PHarness. A
passed qualification for deployed `48c77b7` cannot authorize a newer runtime.
Record the earliest compatible recovery release before enabling hosted build
writes; schema 53 alone does not prove that an older controller understands them.

Before M07 acceptance, demonstrate the actual autonomous source/PR/check/merge/
PipelineRun/registry chain. M08 must add staging digest delivery, actual Argo and
image observation, and bounded application verification. M09 must bind human
approval before production GitOps merge. The [program](../../programs/autonomous-sdlc/ASTRA-00-PROGRAM.md)
owns milestone status; these gates remain open.
