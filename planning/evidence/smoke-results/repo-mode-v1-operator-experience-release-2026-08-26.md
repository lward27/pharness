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
