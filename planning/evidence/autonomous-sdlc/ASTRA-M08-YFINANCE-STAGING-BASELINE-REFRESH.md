# ASTRA M08: Refresh the yfinance staging baseline

Status: prepared and validated; rollout and a new observation window remain required.

The existing staging digest `sha256:f1cfc06fcac62d7c37a4d7dc87237e2abe02df0d9c3824a7521c5359058879c1` answers health checks but returns 500 for invalid tickers and 404 for the market route. Its source revision is unproven. Current application main already implements the required validation and market routes, so leaving the old image in staging prevents a truthful baseline for autonomous maintenance.

This change updates only the staging digest to `sha256:33f1a08b74c82fb5dc01ef0ebef8a1fa5e2fc0ac78be17dadd1f74bbf1e319ca`, previously built from merged source `efa6294954b01a089a65419c85542b8fc2f95c83` by real Tekton run `astra-m07-yfinance-efa6294`. That run passed 37 application tests and produced matching source, image URL and digest results. The [preflight](ASTRA-M08-YFINANCE-STAGING-BASELINE-PREFLIGHT.json) independently rechecks the current main revision, registry manifest/config hashes, verified TLS, Linux AMD64 and OCI source labels. No rebuild or generated application patch is involved.

Kustomize comparison changes only the staging Deployment image; network policy, telemetry configuration, Service, resource limits and production manifests are unchanged. Strict server dry-run passed. Argo's existing automatic reconciliation must apply the exact merged GitOps revision, and native deployment, functional, five-minute metrics/log and health-trace checks must then be captured against the new Pod identity.

If the staging refresh fails, stop further staging work and retain the failing evidence. A reviewed GitOps revert can restore the prior operational state, but the old digest must not be labelled a verified healthy automatic rollback baseline. Production requires a separate human approval before any GitOps change. This platform repair does not count as the autonomous M08 delivery demonstration or either M11 maintenance WorkItem.
