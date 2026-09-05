# ASTRA: Hosted workflow configuration and compatibility

Status: versioned contract with partially implemented delivery. The
[program](../programs/autonomous-sdlc/ASTRA-00-PROGRAM.md) owns current deployment,
qualification, activation and acceptance status. The compatible schema-53 reader
was verified on 2026-09-05; hosted creation remains disabled. A registered binding
or compatible reader is not autonomous-delivery acceptance.

## One request and one saved authorization

The product-scoped WorkItem API is the future submission path. Readiness resolves
registered repository defaults and server-owned delivery configuration. Its hash
includes the selected workflow policy. Creation records that exact policy, making
the application repository, GitOps repository, deployment targets, coding profiles,
limits and rollback permission inspectable for the lifetime of the WorkItem.
The client cannot supply a replacement workflow policy.

The snapshot uses `pharness.dev/hosted-workflow/v1alpha1`. Its Lucas delivery binding
uses `pharness.dev/lucas-delivery-binding/v1alpha1`. A WorkItem owns one mutable
application repository; delivery may change the separately authorized GitOps
repository. Related repositories are pinned read-only context. Production approval
is fixed before production GitOps merge, and one rollback to a preceding verified
deployment is the maximum permission. No configuration grants incident initiation,
unbounded repair, an additional coding backend or generic platform adapters.

Canonical stages remain discover, plan, implement, test, verify, source_delivery,
release and observe. Repair is the existing bounded implementation correction.
A successful source merge keeps hosted work open. An invalid source merge can
terminate the request as failed, with downstream stages unfulfilled. A hosted row cannot complete successfully until all eight
stage outcomes are successful; M07–M09 additionally bind the individually
inspectable source, build, staging, approval and production evidence. The stage
count alone is not a complete evidence chain.

## Authoritative configuration

`deploy/helm/pharness/values.yaml` owns `hostedWorkflow`. When enabled, the API Deployment passes
it as `PHARNESS_HOSTED_WORKFLOW_CONFIG_JSON`. The reader default and committed
Helm setting are disabled. The chart currently declares the yfinance binding. The frontend Pipeline and
production contract were also registered through the live schema-53 API; its
complete hosted binding still requires integration. [Frontend registration](../evidence/autonomous-sdlc/ASTRA-M05-FRONTEND-CONTRACT-REGISTRATION.json)
records the returned identities. These declarations do not enable execution. Never insert guessed database IDs or silently substitute
another repository, environment or Pipeline.

The schema-51 API registered the existing yfinance Pipeline, both staging targets,
and the protected yfinance production target through its supported contract APIs.
[Registration evidence](../evidence/autonomous-sdlc/ASTRA-M05-CONTRACT-REGISTRATION.json)
contains returned identifiers, exact documents and the accepted Finance generation
`dbgen_finance_20260827`. These are metadata declarations. Registration launched no
WorkItem, build, deployment, approval or cluster mutation. Frontend production
contract declaration is now recorded separately. It does not grant production
approval or implement frontend release execution.

Each binding names the Product and repository IDs, canonical source URL/main,
`lucas_engineering`, PipelineContract ID, untagged registry repository, canonical
GitOps URL/main, distinct staging/production contract IDs and Kustomization paths,
and rollback permission. The API verifies registered repository identity, separate
GitOps worker allowlists, active contract snapshots, namespace separation and
qualified profile hashes before accepting hosted work. Source, image, pipeline,
GitOps paths, Argo applications and service coordinates must belong to the same
reviewed Finance application; individually valid contracts cannot be mixed.
Empty/malformed bindings
block enabled creation rather than falling back to source-only semantics.

## Qualification and later stages

All five V2 stage profiles require the exact frozen suite, runtime, policy and
model-target evidence. Builder and Repair qualification require two independent runs.
Fixture-specific qualification tool hashes and actual WorkItem tool hashes remain
separate, with both retained in the evidence. Actual acceptance command/evidence
constraints specialize the Run binding when it starts.

Later stages and resumed runs validate the original policy, compiled profiles,
planned gateway selections and limits. Changing defaults cannot redirect saved
work; changing a required implementation or disabling its gateway fails visibly.
Disabling **new submissions** does not change the authorization of existing work.
Deterministic Test requests no model access. Test fixtures do not establish a
qualified live provider or autonomous delivery.

## Durable source merge boundary

The hosted source controller preserves the original branch/PR publication
permission and records separate `pharness.dev/hosted-source-merge/v1alpha1`
authority in its existing operation. That authority binds the approved ChangeSet,
policy, original base, exact PR head and branch, Finance repository, required CI
identity and original source-wait deadline. It cannot target the GitOps repository.

The isolated Git writer validates strict, administrator-enforced branch protection,
required CI and exact source. A durable admission record permits one provider merge
attempt. Lost admission or merge acknowledgements lead to observation, not another
merge attempt. Pause/cancel and expired or stale evidence withhold new admission;
late outcome receipts remain recordable. GET requests never grant an attempt.

A worker receipt remains a claim pending independent provider observation. The
observer reads the actual merge commit's parent commits and tree. Hosted source
success requires matching original base/head ancestry, fresh checks and a recorded
admission; source-only history retains its prior contract. Successful hosted source
work remains open for the separate build, deployment and runtime gates. See the
[M07 merge evidence](../evidence/autonomous-sdlc/ASTRA-M07-GUARDED-SOURCE-MERGE.md)
for implementation, validation and remaining gaps.

## Reader rollout and rollback floor

Migration 0052 introduced hosted policy fields and completion guards; 0053 added
durable reconciliation, operations and claims. The live Finance history and schema
were verified after deploying the complete `48c77b7` image set. The
[compatible controller release](../evidence/autonomous-sdlc/ASTRA-M06-COMPATIBLE-CONTROLLER-RELEASE.md)
is the current schema-53 recovery floor. Schema-52 binaries, including the retained
`2249950` release, cannot be deployed against that database.

The new source-merge execution kind also requires compatible API and worker
readers. Before enabling these workflow writes, release all required images from
one merged source revision and record the new minimum compatible rollback set.
Do not roll back to a reader that could misunderstand the recorded hosted effects.

The chart omits hosted configuration while creation is disabled. Preserve that
boundary through preparatory releases and finish active qualification/ordinary
Runs before replacing their runtime. Historical rollout comparisons are dated
checks, not a permanent claim that later renders are identical.

Hosted cutover requires qualified gateway/V2 execution, usable contracts, durable
progression and accepted delivery behavior. Only then enable creation through the
reviewed GitOps release. The old unscoped creation route returns an explicit
retirement response; historical reads remain available. This document does not
waive the separate M05–M11 acceptance checks or human production approval.

## Retained implementation history

The [schema-52 reader evidence](../evidence/autonomous-sdlc/ASTRA-M05-COMPATIBLE-READER-RELEASE.md)
remains useful migration history, not the current rollback target. Hosted writes
remain disabled until the program gates pass. Qualification currently binds the
exact compiled API revision: a later release requires matching qualification
before new hosted creation. Prior qualifying evidence is retained and is not
silently transferred to a different runtime.
