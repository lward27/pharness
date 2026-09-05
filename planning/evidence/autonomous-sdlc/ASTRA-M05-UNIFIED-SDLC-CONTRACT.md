# ASTRA M05: Hosted contract implementation evidence

Status: compatible reader deployed; creation and stage-authority preparation tested; **milestone acceptance remains open**.
Observed 2026-09-05. Implementation base: PHarness
`fd740927110366a983de6bb0d3bc6c576577708b`, including M03 and the M04 evaluator
scratch-lifecycle repair. The compiled cluster runtime is now
`2249950d225a4632b24235c2b6f2d8469a774243`, deployed through
[PR 333](https://github.com/lward27/pharness/pull/333), release commit
`8ca88f32e3d50f8430cf5a486912ebe6d00a392d`. Its seven immutable images and native
bundle are verified. The live Finance database is on migration 0052 with historical
records preserved. Hosted creation and Coding Reliability V2 remain disabled.
[Release and rollback floor](ASTRA-M05-COMPATIBLE-READER-RELEASE.md).

## Implemented and tested

- Introduce the finite `pharness.dev/lucas-delivery-binding/v1alpha1` and immutable
  `pharness.dev/hosted-workflow/v1alpha1` policy on the existing WorkItem. Bind one
  application repository, the separately authorized GitOps repository, exact
  PipelineContract and DeploymentContract documents, all five V2 profiles,
  gateway selections, existing execution ceilings, one correction, and rollback
  permission. Production approval is fixed before GitOps merge.
- Resolve delivery configuration from the registered Product/Repository. Reject
  unknown configuration fields, foreign cluster bindings, ambiguous paths,
  mismatched repositories, unavailable contracts, or unqualified profiles.
  Readiness may default to the registered source commit and declared acceptance
  commands; the response and authorization hash expose the resolved choices.
- Cross-check the source repository against its finite Finance pipeline, image,
  GitOps repository/paths, Argo applications, Deployment, Service, port and probe.
  Independently valid contracts for different applications cannot be combined
  into one authorization. Preserve `prune=false` and `force=false`. Both reviewed
  Finance applications are supported; broader application bindings remain a
  future extension rather than an arbitrary deployment route.
- Add policy/hash fields with migration 0052. Existing records retain null policy
  and source-only semantics. Reject policy/scope/budget changes and malformed
  policy pairs. A hosted row cannot complete without all eight successful stage
  outcomes. Release and Observe cannot be inserted as inapplicable.
- Keep a successfully merged hosted WorkItem open for build, deployment, approval,
  and runtime evidence. Preserve repeated-callback safety and original legacy
  closure behavior. Return an explicit retirement response for unscoped creation
  after hosted cutover, including when the old creation flag is set.
- Add `workflow_kind`, policy, and policy hash to compatible WorkItem reads.
  Retain the stored `mode=repo` discriminator for existing readers and migrations.
  `workflow_kind=source_only` describes historical scope; it is not hosted success.
- Project hosted delivery from the saved authorization in WorkItem detail. List
  required build, staging and production steps, their exact contract IDs, the
  production approval boundary and required evidence. Unexecuted release and
  observation remain pending with no invented resource evidence. Historical
  source-only detail retains its original inapplicable tail. The source-merge
  regression covers both the stored result and this console-facing response.

## Stage authority and qualification integrity

Planner, Builder, Repair, Test diagnosis and Verify now use the saved profiles
and explicit gateway policies. Stage preparation checks the immutable policy,
compiled profile, configured target/policy hashes and planned binding before
creating new execution work. Native-host overrides and sticky native workspaces
are rejected for hosted work. Disabling new submissions does not rewrite an
existing WorkItem's contract; disabling its gateway/V2 execution fails closed.

A prepared/resumed Run is checked again before model selection or model-grant
issuance. It must carry its workflow hash, planned gateway selection, profile,
and original limits. Changed defaults cannot select another policy; missing
markers cannot fall back to the direct provider. The saved live binding is
specialized with actual WorkItem acceptance/evidence tools when the Run starts.
Existing exact Run selections are reused on resume. Deterministic Test explicitly
requests no model selection. Repair retains the existing V2 convention: the
repair profile executes a bounded implementation pass, with distinct prompt,
policy and evidence, without inventing a new lifecycle stage.

Qualification now reconstructs the existing frozen suite binding using the same
helper as the evaluation dispatcher. Fixture-specific tool schemas are compared
against that qualification binding, while the live WorkItem binding remains
separate. This corrects an impossible comparison between frozen-fixture and live
tool hashes. Readiness requires matching runtime, suite, profile, policy and target
hashes, and two Builder and Repair attempts in their qualifying reports. The qualification hash
algorithm and frozen suite are unchanged; deterministic fixture rows in unit tests
are explicitly not live qualification proof.

The finite production frontend DeploymentContract can be declared with exact
`Deployment/finance-frontend`, `finance-frontend:8080/` and a required health probe.
Unknown applications, cross-environment targets and altered service coordinates
are rejected. Declaration does not widen the legacy protected-yfinance executor;
M09 still owns hosted approval and production promotion.

## Validation

451 distinct tests passed across API/admin, configuration, core and store suites:
1 admin, 236 API, 16 configuration, 144 core unit, 6 existing core integration,
2 hosted contract integration and 46 store tests. The four-crate run passed 448
tests; after the final readiness and cross-application checks were added, all 236
API tests and the admin test passed again. Unchanged core/configuration/store results are not counted
twice. No production database was used.
The earlier 441-test run is retained as historical preparation evidence, not added
to this total. The SQL migration-count assertion was updated to 52.

Coverage includes immutable scope/limits; paused and legacy records; schema-51
upgrade preservation; repeat migration; incompatible older SQLx readers; retired
creation; invalid/unqualified readiness without inserted work; hosted source merge
remaining nonterminal; duplicate source completion; qualification/live-tool hash
separation; the two-run requirement; changed defaults/profile/backend/limits;
missing resume markers; successful repair selection/resume; and frontend declaration
without legacy production authorization. The readiness response reports the exact
Planner binding saved in the policy, even when the worker default differs; a
contract that disables its health probe is rejected.
The same API regression also supplies explicitly synthetic, current repository and
qualification evidence, then exercises successful readiness and creation. The
stored policy matches the readiness response, creation reports `hosted_sdlc`, and
only the controller's Discover outcome exists. No workspace, coding job or
deployment is invented. This is positive local API coverage, not positive live
provider acceptance. Readiness describes recording authorization until M06 adds
durable scheduling.
The positive API fixture uses the real Finance coordinates with synthetic
repository/qualification records and a disabled fake Git executor. Fourteen
otherwise well-formed mismatches and destructive sync flags are rejected; these
tests do not contact or mutate the application repositories.

Clippy passed for all four changed crates and targets with warnings denied.
Formatting and architecture boundary checks pass, including all five dependency
parser tests. The stage-chain split initially created a circular import through a
re-export; callers now import the owning authorization module directly. The
unnecessary re-export was removed, rather than weakening the boundary check.
The last import-only correction also passed the API compiler/linter with warnings denied.

After merging the M04 protocol and stage-report repairs, combined source
`1406174ae6a41a35beeef943d799bb86a78c4441` passed the full workspace:
**616 Rust tests**, full workspace/all-target Clippy with warnings denied,
formatting, and architecture checks. The UI build and **73 unit tests** passed.
**103 distinct browser checks passed** across desktop and phone-width projects;
the mobile copy of the real-server journey is intentionally skipped. That journey
initially exceeded its setup timeout while compiling alongside another build. A
separate fixture build followed by the unchanged real-server test passed in 31.7
seconds. No test limits or screenshot baselines were changed. An initial shared
Cargo cache collision was also resolved using a worktree-local target directory.
[Combined validation evidence](ASTRA-M05-COMBINED-SOURCE-VALIDATION.json) records
per-binary results, commands, log hashes, and the local Node 24 versus release
Node 22 distinction. These results supersede the earlier subset counts; they are
not added to them and do not constitute M04 or M11 acceptance.

The JSON fixture under `crates/pharness-core/tests/fixtures` is test data. It is
not a qualified provider profile or live delivery evidence.

## Live declaration preparation

The existing schema-51 API registered the yfinance Pipeline, yfinance staging,
yfinance production and frontend staging contracts with actual returned IDs.
[Registration evidence](ASTRA-M05-CONTRACT-REGISTRATION.json) preserves their exact
coordinates and states the metadata-only effects. No WorkItem, PipelineRun,
deployment intent, approval or cluster mutation was created.

The prepared Helm values bind yfinance to those IDs. With hosted creation
disabled, the API uses its safe empty default and the complete rendered chart
matches current main byte for byte. The environment entry appears at cutover;
publishing reader source therefore does not restart the current evaluator API.
[Rollout comparison](ASTRA-M05-DISABLED-ROLLOUT-COMPARISON.json) records this boundary. Frontend production
registration awaits the M05 compatible API; its actual Pipeline and registration
remain M07. The chart renders and lints with the disabled binding; the API Deployment passes
server-side dry-run in namespace `pharness`. See the
[configuration and rollback guide](../../design/ASTRA-HOSTED-WORKFLOW-CONFIGURATION.md).

## Compatibility and cutover

`PHARNESS_HOSTED_WORKFLOW_CONFIG_JSON` supplies server-owned configuration. Its
safe default is `{"enabled":false,"bindings":[]}`. New hosted policy snapshots
are part of the readiness hash; the client cannot supply an arbitrary policy.
When enabled, missing configuration blocks creation rather than choosing a
source-only fallback.

Migration 0052 is additive but raises the executable-reader floor: the previous
SQLx migrator rejects an applied migration it does not know. **Even a deployment
with hosted writes disabled cannot subsequently roll back to the schema-51
binary.** The minimum rollback release must include 0052 and the hosted readers.
The 2249950 release is the retained compatible floor, recorded before
applying 0052 and now verified live. Do not remove the migration or reset Finance's database generation.

Creating new hosted work currently requires current-runtime gateway qualification
as well as matching profile, policy, and target hashes. The earlier evaluation on
runtime 83a2689 failed; fresh M04 qualification must follow the next release containing the prompt clarification.
Results for either runtime cannot silently qualify an unbuilt M05 API revision.
Keep qualification provenance explicit at cutover.

## Remaining acceptance work

1. M02's platform coordinates, restored backend baseline and staging checks are
   available. Finish the remaining frontend runtime contracts and M04 qualification.
   Deterministic fixtures do not establish positive live hosted creation.
2. Stage-entry and resume enforcement now pass deterministic tests. Validate the
   actual deployed reader/gateway/worker path and saved policy under real hosted
   creation before closing the milestone. The reader is deployed; positive live hosted creation still awaits qualified execution and the delivery gates.
3. Represent individually inspectable build/staging/production evidence through
   the existing release/effect records and test completion against those records.
   The database's eight-stage guard is a necessary floor, not a complete proof of
   the delivery evidence chain implemented by M07–M09.
4. The compatible reader and live schema/generation preservation are verified.
   Finish positive creation under qualified bindings and every retirement route
   before enabling hosted writes. Historical pending and closed records remain unchanged.
5. Complete M06's durable reconciliation before describing routine progression
   as autonomous. Browser-independent behavior is not provided by this slice.

Do not enable hosted creation or close M05 based on this document's test count.
The [milestone](../../programs/autonomous-sdlc/ASTRA-05-UNIFIED-SDLC-CONTRACT.md)
and [master](../../programs/autonomous-sdlc/ASTRA-00-PROGRAM.md) retain their gates.

## Real Finance data compatibility check (2026-09-05 13:39 UTC)

A consistent read-only snapshot of the live schema-51 database is retained at
`/data/archives/ASTRA-pre-0052-20260905` on
`pharness-api-data-finance-20260827`. Its 21,204,992-byte database has SHA-256
`fbc7e2873e71202ae90fcb1fe28e6f160e189e9adc5c29ab395cb39ce7c29d3f`.
The archive and source pass integrity checks. This snapshot is on the existing
volume; it is not a separate disaster-recovery backup. See
[archive evidence](ASTRA-M05-DATABASE-ARCHIVE-VERIFIED.json).

The published runtime image from source `2249950` started in draining mode against
an isolated temporary copy and applied migration 0052. The migration container had
no access to the live PVC; copy/verification containers mounted it read-only.
The API health check passed. All historical WorkItem status, source, closure and
version fields matched the archive. All new workflow policy fields remained null.
Counts were preserved: 14 WorkItems, 82 Runs, 102 stage executions/outcomes,
83 evidence validations, 260 audit records and four retention holds. Database
generation was identical; the original archive remained on schema 0051. See
[clone migration evidence](ASTRA-M05-CLONE-MIGRATION-VERIFIED.json).

The later live rollout is verified at 14:07 UTC against the exact Argo revision
and all five Deployment image identities, with zero restarts. At 14:08 UTC a
separate read-only Job confirmed live migration 0052, integrity and the same record
counts and historical WorkItem fields. The generation and four retention holds
remain unchanged. See [live deployment evidence](ASTRA-M05-READER-RELEASE-OBSERVED.json)
and [live database comparison](ASTRA-M05-LIVE-DATABASE-VERIFIED.json).
Neither check created hosted work or closed M05/M04 acceptance.

## Frontend declarations completed after compatible rollout

At 16:23 UTC the existing frontend Pipeline and production target were registered through the supported schema-53 API. Pipeline contract `pcontract_1788625407693432337` identifies the real `pharness-finance-frontend-build` Pipeline; deployment contract `dcontract_1788625408162216483` identifies `production/apps-prod/finance-frontend`. [Registration evidence](ASTRA-M05-FRONTEND-CONTRACT-REGISTRATION.json) records exact bodies and read-back identities. These metadata writes created no WorkItem, build, deployment intent or production approval. Hosted bindings and runtime-verification/approval gates still require integration.
