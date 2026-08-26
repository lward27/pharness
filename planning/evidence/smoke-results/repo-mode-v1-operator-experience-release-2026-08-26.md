# Repo Mode V1 operator-experience release evidence

Observed: 2026-08-26

Scope: PHarness Repo Mode V1 operator-experience rollout on Kubernetes context
`lucas_engineering` and the existing yfinance Product evidence. This record
contains no credential, Secret, authorization-header, or kubeconfig material.

## Provenance

| Item | Observed value |
| --- | --- |
| Implementation PR | `#161` |
| Merged source revision | `069ce56078da1081c01570844e792bda8a95c9ee` |
| Flag-off release PR / revision | `#162` / `cbfccd079ef082a9425546399f1ac1cc7d109d0e` |
| Flag-enable PR / deployed revision | `#163` / `7c27b6e29905a17c8aeb7eb63ee386646738fc04` |
| Runtime image | `registry.lucas.engineering/pharness-runtime@sha256:1bb00ca024d097e0c1f03c08d0dc9c8e43902a7204926fb6550aa44ea9f931e4` |
| UI image | `registry.lucas.engineering/pharness-ui@sha256:57ea5473989cc158faf82a0e7dbbb66d2bf58d6552ed5e5e6dfda420c792eec2` |
| Python runner image | `registry.lucas.engineering/pharness-python-runner@sha256:e641183ef11d76ffc67074142a945a546d1f110153c8920ca985753c9ebaecdf` |
| SQLite backup | `/data/backups/pharness-before-069ce560-20260826T0335Z.db`, 7,221,248 bytes, integrity `ok` |

Each OCI artifact reported `linux/amd64`, source
`https://github.com/lward27/pharness`, and revision
`069ce56078da1081c01570844e792bda8a95c9ee`.

## Deterministic acceptance

- `cargo fmt --all -- --check`: passed.
- `cargo test --workspace`: passed across the full workspace.
- `cargo clippy --workspace --all-targets -- -D warnings`: passed.
- UI production build: passed.
- Vitest: 36 passed.
- Playwright: 81 passed and one intentional mobile skip. This includes the
  complete real API/controller/temporary-SQLite/test-provider journey.
- Automated accessibility assertions passed on every primary Repo Mode route.
- Helm lint, schema, template, immutable `:latest` scans, and Kubernetes
  server-side dry-run passed.

## Rollout observations

The flag-off release reached Argo `Synced/Healthy` at
`cbfccd079ef082a9425546399f1ac1cc7d109d0e`. Live API, both egress proxies, and
UI Pods reported image IDs equal to the declared digests. API and UI revisions
matched the compiled source revision and the feature read model reported
`ui_enabled=false`. Browser characterization rendered the legacy Triage shell.

The separate enable release reached Argo `Synced/Healthy` at
`7c27b6e29905a17c8aeb7eb63ee386646738fc04`. The feature read model reported
`ui_enabled=true`. All eight routes loaded without alerts:

1. Overview
2. Products
3. Repositories
4. WorkItems
5. Agents
6. Releases
7. Insights
8. Settings

Phone-width verification showed one content column, hidden slide-over
navigation, and no horizontal overflow.

## Live yfinance characterization

Completed WorkItem `witem_01a03a99600479a095e3533c5f61c7b6` rendered:

- Lifecycle position: Source Delivery.
- SourceDeliveryIntent: `succeeded`.
- Pull request: `lward27/yfinance_wrapper#4`.
- Required checks: `passing`.
- Exact merge provenance present.
- Release: controller-recorded `inapplicable`.
- Observe: controller-recorded `inapplicable`.
- Closure reason: manual merge matched the approved head and fresh
  authoritative required checks.

The legacy `0/5` delivery warning was absent.

## Honest residual state

After rollout, isolated capability verifications reported `stale` because their
15-minute evidence windows had expired. The UI displayed that state and exact
refresh actions instead of claiming availability. This does not invalidate the
read-only release characterization; creation of new coding work remains gated
until the relevant capability checks are refreshed.

The single-replica API replacement produced a brief upstream refusal window in
the UI proxy during rollout. It cleared after the API became ready; subsequent
route checks completed without alerts. No direct Deployment patch, rollout
restart, database down-migration, or evidence deletion was used.

## Rollback

Set `features.repoModeV1.uiEnabled` back to `false` in Helm values through a
reviewed GitOps commit. The flag-off release at
`cbfccd079ef082a9425546399f1ac1cc7d109d0e` and the pre-release SQLite backup
remain available. Rollback requires no database down-migration.

## Corrective acceptance release

The original record above remains preserved as historical evidence. A later
same-day audit and live acceptance produced this corrective release:

| Item | Accepted value |
| --- | --- |
| Read-model and UI completion PR | `#165` |
| Responsive acceptance correction PR | `#168` |
| Accepted source revision | `c3638afa6d14819adca82006630858935c616172` |
| Final flag-off PR / Argo revision | `#169` / `8662e27e1d1ad944dcc93af75d37b398adc03d95` |
| Final flag-enable PR / Argo revision | `#170` / `05f0c079c43c8f3491ba76c451356d1d55324767` |
| Runtime image | `registry.lucas.engineering/pharness-runtime@sha256:5a304073220f370433f615e73ea914a36e306e4cbd61a64a55562ff349135c7f` |
| UI image | `registry.lucas.engineering/pharness-ui@sha256:78930c2bff88497e72927c788ffbdad23f15d6c38126bc9296028b043fb1e3f6` |
| Python runner image | `registry.lucas.engineering/pharness-python-runner@sha256:62474fc976a185b26fac6bd3439d58f6fe88f0c8bb4e5a76ee06f06c2a29a921` |
| SQLite backup | `/data/backups/pharness-before-c187cc5-20260826T230516Z.db`, 7,221,248 bytes, integrity `ok` |

All three accepted artifacts report `linux/amd64`, source
`https://github.com/lward27/pharness`, and the exact accepted source revision.
The final API and UI Pods reported image IDs equal to the declared digests;
platform readiness reported matching API/UI revisions and the runner profile
reported the accepted digest and revision.

The corrective implementation passed Rust formatting, full workspace tests,
all-target Clippy with warnings denied, Helm validation, immutable manifest
scans, exact-cluster server-side dry-run, and OCI inspection. The accepted
source passed the production UI build, 43 Vitest tests, and 85 Playwright tests
with one intentional mobile-only skip. The Playwright set includes the real
API/controller/SQLite/provider journey and accessibility coverage.

The initial corrective live rollout revealed one responsive defect: long
server-owned attention text expanded the Organization Overview by 47 pixels on
desktop and 67 pixels at phone width. It was fixed through PR `#168`, given an
explicit overflow regression, rebuilt as a fully aligned three-image set, and
promoted through the separate flag-off and flag-enable releases above.

Final live browser acceptance loaded Overview, Products, Repositories,
WorkItems, Agents, Releases, Insights, and Settings at both 1440-pixel and
390-pixel widths. Every route had a visible server-backed heading, zero alerts,
zero page exceptions, and zero horizontal overflow. Completed yfinance
WorkItem `witem_01a03a99600479a095e3533c5f61c7b6` showed Source Delivery
`succeeded`, Release and Observe `inapplicable`, and no `0/5` or delivery
reconciliation warning.

The isolated capability and runner verification windows were expired at final
observation. Readiness therefore remained honestly `stale`; this is a
correctness-preserving gate on new work, not a hidden release failure. Rollback
is still the reviewed GitOps change setting `features.repoModeV1.uiEnabled` to
`false`; no database down-migration or evidence deletion is required.
