# ASTRA M05: Hosted contract implementation evidence

Status: tested reader, creation, and stage-authority preparation; **milestone acceptance remains open**.
Observed 2026-09-05. Implementation base: PHarness
`fd740927110366a983de6bb0d3bc6c576577708b`, including M03 and the M04 evaluator
scratch-lifecycle repair. The compiled cluster runtime remains
`83a2689c877a3f48688d1d457c34e83474698c46`; none of the new Rust code or migration
has been deployed. Hosted creation remains disabled by default.

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
hashes, and two builder attempts in the qualifying report. The qualification hash
algorithm and frozen suite are unchanged; deterministic fixture rows in unit tests
are explicitly not live qualification proof.

The finite production frontend DeploymentContract can be declared with exact
`Deployment/finance-frontend`, `finance-frontend:8080/` and a required health probe.
Unknown applications, cross-environment targets and altered service coordinates
are rejected. Declaration does not widen the legacy protected-yfinance executor;
M09 still owns hosted approval and production promotion.

## Validation

448 tests passed across API/admin, configuration, core and store suites:
1 admin, 233 API, 16 configuration, 144 core unit, 6 existing core integration,
2 hosted contract integration and 46 store tests. No production database was used.
The earlier 441-test run is retained as historical preparation evidence, not added
to this total. The SQL migration-count assertion was updated to 52.

Coverage includes immutable scope/limits; paused and legacy records; schema-51
upgrade preservation; repeat migration; incompatible older SQLx readers; retired
creation; invalid/unqualified readiness without inserted work; hosted source merge
remaining nonterminal; duplicate source completion; qualification/live-tool hash
separation; the two-run requirement; changed defaults/profile/backend/limits;
missing resume markers; successful repair selection/resume; and frontend declaration
without legacy production authorization.

Clippy passed for all four changed crates and targets with warnings denied.
Formatting and architecture boundary checks pass, including all five dependency
parser tests. The stage-chain split initially created a circular import through a
re-export; callers now import the owning authorization module directly. The
unnecessary re-export was removed, rather than weakening the boundary check.
The last import-only correction also passed the API compiler/linter with warnings denied.

The JSON fixture under `crates/pharness-core/tests/fixtures` is test data. It is
not a qualified provider profile or live delivery evidence.

## Live declaration preparation

The existing schema-51 API registered the yfinance Pipeline, yfinance staging,
yfinance production and frontend staging contracts with actual returned IDs.
[Registration evidence](ASTRA-M05-CONTRACT-REGISTRATION.json) preserves their exact
coordinates and states the metadata-only effects. No WorkItem, PipelineRun,
deployment intent, approval or cluster mutation was created.

The prepared Helm values bind yfinance to those IDs and expose the configuration
through the API environment with hosted creation disabled. Frontend production
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
No such release is deployed yet; record its immutable release identity before
applying 0052. Do not remove the migration or reset Finance's database generation.

Creating new hosted work currently requires current-runtime gateway qualification
as well as matching profile, policy, and target hashes. The live M04 evaluation
started on runtime 83a2689 is a baseline for that runtime; it is not evidence for
an unbuilt API revision. Keep qualification provenance explicit at cutover.

## Remaining acceptance work

1. M02's platform coordinates, restored backend baseline and staging checks are
   available. Finish the remaining frontend runtime contracts and M04 qualification.
   Deterministic fixtures do not establish positive live hosted creation.
2. Stage-entry and resume enforcement now pass deterministic tests. Validate the
   actual deployed reader/gateway/worker path and saved policy under real hosted
   creation before closing the milestone. No new M05 code has been deployed.
3. Represent individually inspectable build/staging/production evidence through
   the existing release/effect records and test completion against those records.
   The database's eight-stage guard is a necessary floor, not a complete proof of
   the delivery evidence chain implemented by M07–M09.
4. Deploy a compatible reader release before any hosted writes. Verify live
   schema/generation preservation, paused and partially completed reads, positive
   creation under qualified bindings, and all retirement routes.
5. Complete M06's durable reconciliation before describing routine progression
   as autonomous. Browser-independent behavior is not provided by this slice.

Do not enable hosted creation or close M05 based on this document's test count.
The [milestone](../../programs/autonomous-sdlc/ASTRA-05-UNIFIED-SDLC-CONTRACT.md)
and [master](../../programs/autonomous-sdlc/ASTRA-00-PROGRAM.md) retain their gates.
