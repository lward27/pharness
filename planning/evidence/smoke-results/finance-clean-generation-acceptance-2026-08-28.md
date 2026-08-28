# Finance clean-generation acceptance — 2026-08-28

This record freezes the acceptance evidence for the PHarness clean-state reset, Finance Product model, and data-lifecycle milestone. Runtime state may change later; revalidate before relying on these values operationally.

## Accepted release

- GitOps revision: `3f523ac15c839aa659c5afa2029e49de24fccfd1`
- Annotated tag: `v5-finance-clean-generation`
- Compiled source revision: `e31349920873b86395514fb2977ae7d4480eff4f`
- Runtime: `registry.lucas.engineering/pharness-runtime@sha256:c9f2ef4016544a22f91aeb3a6a420eb09a0f2e5423bdfdf3e49f1e5eb36c0c09`
- UI: `registry.lucas.engineering/pharness-ui@sha256:0bb7d51e7637e085b31032327129e27ce569640ea12166fab15bfe8274dc1cbf`
- Python runner: `registry.lucas.engineering/pharness-python-runner@sha256:6f96f317b55a1d69efe0ec439f687182f0eff2a4adeb4180bdc61acdff9b245e`

All three artifacts were built by successful Tekton PipelineRuns from the same full source SHA. Registry inspection confirmed Linux/AMD64 plus exact OCI revision and source labels. Argo reported `Synced/Healthy` at the GitOps revision, API/UI compiled revisions matched, both live pod `imageID` values matched the declared digests, and both pods were ready with zero restarts.

## Database and archive boundary

- Active generation: `dbgen_finance_20260827`
- Schema version: `0049`
- Operational mode: `normal`
- Legacy WorkItem creation: disabled
- Archived generation record: `archive_1787837361506841280`
- Archived database claim: `pharness-api-data`
- Verified archive claim: `pharness-data-archive-legacy-20260826`
- Deletion eligibility: `1789133361000` Unix milliseconds

The active API mounts only `pharness-api-data-finance-20260827`. The legacy database and archive claims are retained with `Prune=false`, are not mounted by a running pod, and have no deletion receipt. Their eventual deletion remains a separately confirmed operation after eligibility.

## Finance Product

The clean generation contains one Finance Product, six Services, and five globally registered Repositories:

- `lward27/finance_app_database_service`
- `lward27/finance-frontend`
- `lward27/yfinance_wrapper`
- `lward27/scraper_manager`
- `lward27/lucas_engineering`

Application Repositories have reviewed source bindings. The shared GitOps Repository is bound only through reviewed Finance-relative delivery, integration, and automation scopes. The frontend and GitOps Repository remain honestly unavailable for Repo Mode coding; no Node runner, multi-Repository mutation, deployment, or runtime-health claim was introduced.

## Supervised Repo Mode acceptance

Database API:

- WorkItem: `witem_01a046224e277041bd03d58d6f78e08f`
- Result: `completed`
- Source PR: `lward27/finance_app_database_service#3`
- Merge commit: `015f720176154921533b88af1526cbd5230e706a`
- Raw-evidence hold: `rethold_1787885507230838288`

Scraper manager:

- WorkItem: `witem_01a046988deb72319b80871151c1f245`
- Result: `completed`
- Source PR: `lward27/scraper_manager#4`
- Approved head: `cfce0a35e8e25fa9f94ef8a795e5e2149fc1e24a`
- Merge commit: `311d26be08f6ac54649b248b6b3378f06ada6e05`
- Raw-evidence hold: `rethold_1787894715352643535`

Both WorkItems completed Planner → Builder → Tester → Verifier, passed their contract-declared acceptance commands, produced controller-sealed outcomes, used exact SourceDeliveryIntents, recorded fresh authoritative provider-check observations before and after manual merge, and closed Source Delivery successfully. Release and Observe were controller-sealed as inapplicable.

Four abandoned pre-acceptance WorkItems and one paused Run were durably cancelled rather than deleted. The cancellation smoke exposed and fixed support for Repo Mode `proposed` and `waiting_external` lifecycle states. Finance ended with zero current WorkItems and zero current AgentRuns; all six WorkItems remain visible in History.

## Retention acceptance

- Preview: `retpreview_1787896277494085532`
- Generation: `dbgen_finance_20260827`
- Policy: `pharness.dev/retention-policy/v1alpha1`
- Eligible workspaces: `0`
- Eligible Runs: `0`
- Eligible capability verifications: `0`
- Active WorkItem holds: `2`
- Automatic daily execution: enabled
- Preview-only mode: disabled after acceptance
- Retention receipts at acceptance: `0`

The preview was reviewed without execution before automatic policy execution was enabled through GitOps. Archive deletion is not part of scheduled execution.

## Deterministic validation

- `cargo fmt --check`
- `cargo test --workspace`
- `cargo clippy --workspace --all-targets -- -D warnings`
- Helm lint and values-schema validation
- Rendered-manifest scan proving no PHarness `:latest` references
- Exact changed API template accepted by Kubernetes server-side dry-run
- OCI platform and label inspection for runtime, UI, and runner
- Live Argo, Deployment, Endpoint, readiness, database-generation, capability, Product-overview, WorkItem-history, archive, hold, and retention-preview checks

## Follow-up boundary

Do not delete the archived claims until their eligibility date and a new exact destructive confirmation. The next planning entry point is [PHarness Data Model Convergence](../../active/PHarness-Data-Model-Convergence-Entry-Point.md); that milestone has not started.
