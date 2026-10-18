# ASTRA M08: Integrated staging review

Status: reviewed and locally validated; **not deployed or autonomously accepted**.
Integrated source: `6f40cdcd78060dd59fa3292435d5cfd76c08bbe5`.
It incorporates the five M08 slices and merged MiniMax context fix from main.

## Objective result

The full Rust workspace passed **782 tests**, with six explicit live-reader tests
ignored by ordinary CI. The count excludes nested benchmark child tests. Clippy
with warnings denied passed for the affected core, API, worker, configuration,
transport and gateway crates, including all their targets. Formatting, architecture
checks and five dependency-parser checks passed. Helm lint/render passed; the only
lint note is the optional chart icon. [Validation and logs](ASTRA-M08-INTEGRATED-VALIDATION.json)
record exact results and hashes.

The rendered [access/configuration check](ASTRA-M08-INTEGRATED-CHART.json) confirms
three finite read Roles and bindings in apps-staging, apps-prod and argocd. No Secret,
exec, Pod log, write or cluster-wide access is added. Namespace list access cannot be
limited by labels through Kubernetes RBAC; the reader separately validates Finance
selectors and ownership. Mimir uses its distinct endpoint, while legacy Prometheus
inventory retains its existing endpoint. Hosted configuration remains absent from
the API environment and Coding Reliability V2 remains false.

The source review traced the baseline admission, worker write boundary, first
GitOps receipt, candidate identity, runtime window and durable completion. The
original source/build/policy and baseline hashes remain bound. Each potentially
interrupted read is recorded before dispatch, and an uncertain final collection
becomes inconclusive. Duplicate callbacks retain the original projection. A passed
staging operation releases its own locks and leaves the WorkItem open, with no
production authority. No additional merge blocker was found in those examined paths.

## Assessment and limits

This is a substantial improvement in what a green staging result means: one image,
its actual deployment, and a complete bounded evidence window. It also adds a
nontrivial maintenance burden. Baseline and candidate observation share concepts
but have different admission/recovery rules; broad consolidation now would risk
hiding those distinctions. Keep the finite implementation until real Finance
acceptance shows which repeated mechanics can be safely simplified.

An unrelated GitOps revision during a window can still block verification. That is
conservative and explicitly reported; it does not silently adopt different evidence.
Frontend initialization and runtime configuration remain M11 work. The earlier
operator-run five-minute staging evidence proves the native readers, not autonomous
source-to-deployment progression. The current tests are synthetic controller checks.
None substitutes for M04 qualification, M07 autonomous delivery, M08 real staging,
M09 production/recovery, or M11 application acceptance.

## Integration and release

Consolidate the reviewed staging stack into one final main-targeting change after
its independent slices have been reviewed. Retain their source/evidence history.
Wait for the current source-19 evaluation to finish before changing its deployment
configuration. Build and release all required artifacts from one final merged source,
with exact digest pins, observed Argo state and live identities. Include any separately
validated coding-contract correction before that build; this 782-test result applies
only to the revision named above.

No new database migration is required. Preserve schema 54 and the Finance generation.
The v1alpha2 staging writer/API must be released before any such hosted authority is
enabled; record that release as the compatible active-writer floor. Keep hosted
creation disabled until its qualification and acceptance prerequisites pass.

## Integration with the stage-contract correction — 2026-09-06

Merged source `06ead42711492377dba03da67cd9d7ec2dfa29a8` also includes the
M04 reference guards and Planner/Test Diagnosis V2.2 corrections from main
`04dbaeab51d9de939f0743d587c3aa3d10c21a8f`. The complete workspace passed
**786 tests**, with six explicit live readers ignored. Clippy across all eight
affected packages, formatting and architecture checks passed. An initial Clippy
invocation used a nonexistent package name and ran no checks; the corrected run
passed and both outputs are retained. The chart is byte-identical to the prior
render validation. [Combined validation](ASTRA-M08-COMBINED-STAGE-CONTRACT-VALIDATION.json)
records exact changed-source and log hashes.

The implementation remains unreleased and autonomous M08 acceptance remains open.
The owner has now requested an M04 process reassessment; passing local integration
tests does not resolve that qualification-design question. The current source-19
Verifier evaluation remains separate, and no production authority is inferred.
