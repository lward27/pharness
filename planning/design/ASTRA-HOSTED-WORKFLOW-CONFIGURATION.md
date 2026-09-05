# ASTRA: Hosted workflow configuration and compatibility

Status: M05 implementation reference, 2026-09-05. Reader source merged in PR 328
at `2249950`. Its immutable image set is being published; hosted creation remains
disabled and the live schema remains 0051. The actual reader successfully migrated
an isolated copy of the Finance database to 0052; live rollout remains pending.
Read the [program](../programs/autonomous-sdlc/ASTRA-00-PROGRAM.md) and
[M05 evidence](../evidence/autonomous-sdlc/ASTRA-M05-UNIFIED-SDLC-CONTRACT.md)
for current acceptance, rather than treating configuration as proof of autonomy.

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
A source merge keeps hosted work open. A hosted row cannot complete until all eight
stage outcomes are successful; M07–M09 additionally bind the individually
inspectable source, build, staging, approval and production evidence. The stage
count alone is not a complete evidence chain.

## Authoritative configuration

`deploy/helm/pharness/values.yaml` owns `hostedWorkflow`. When enabled, the API Deployment passes
it as `PHARNESS_HOSTED_WORKFLOW_CONFIG_JSON`. The reader default and committed
Helm setting are disabled. The chart currently declares only the verified yfinance
binding; frontend registration completes after the compatible M05 API and M07
Pipeline are available. Never insert guessed database IDs or silently substitute
another repository, environment or Pipeline.

The schema-51 API registered the existing yfinance Pipeline, both staging targets,
and the protected yfinance production target through its supported contract APIs.
[Registration evidence](../evidence/autonomous-sdlc/ASTRA-M05-CONTRACT-REGISTRATION.json)
contains returned identifiers, exact documents and the accepted Finance generation
`dbgen_finance_20260827`. These are metadata declarations. Registration launched no
WorkItem, build, deployment, approval or cluster mutation. Frontend production
contract declaration requires the M05 API change; it does not enable the old
protected-yfinance executor to deploy the frontend.

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

## Reader rollout and rollback floor

Migration 0052 adds policy/hash fields and completion guards while preserving
legacy records, Finance generation, retention and audit history. Legacy WorkItems
retain null policy and explicitly source-only completion semantics. Additive does
not mean the schema-51 executable is rollback-compatible: its SQLx migrator rejects
an unknown applied migration.

Build and retain the immutable compatible reader release first. Record its exact
image set as the earliest safe rollback release **before applying 0052**. Deploy
with `hostedWorkflow.enabled: false`; verify the actual schema/generation and
historical, paused and partial reads. An older image must not misread hosted work
or be used after this migration. Restore forward with the retained compatible
reader if a subsequent release fails; do not remove migration history or reset data.

The chart omits the hosted environment entry while creation is disabled. The
complete disabled Helm output matches current main byte for byte, so publishing
reader source does not restart the running API. Enabling hosted creation later
adds the configuration and intentionally rolls the API. Merge the immutable image
pins only after active qualification and ordinary Runs reach a safe boundary.
[Rollout comparison](../evidence/autonomous-sdlc/ASTRA-M05-DISABLED-ROLLOUT-COMPARISON.json)
records this check. This replaces the earlier unconditional environment entry,
which would have restarted the API during the current evaluation.

Hosted cutover requires qualified gateway/V2 execution, usable contracts, durable
progression and accepted delivery behavior. Only then enable creation through the
reviewed GitOps release. The old unscoped creation route returns an explicit
retirement response; historical reads remain available. This document does not
waive the separate M05–M11 acceptance checks or human production approval.
