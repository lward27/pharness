# ASTRA M08: Finance source and deployed baseline differ

Observed on September 5, 2026, against staging only. This updates the current-baseline account without rewriting historical evidence.

The pinned yfinance image `sha256:f1cfc06fcac62d7c37a4d7dc87237e2abe02df0d9c3824a7521c5359058879c1` is healthy and its Deployment, current ReplicaSet, Pod image identity and ready Service endpoint agree with Argo. Those observations do not prove the current application's behavior.

The new bounded probes found:

- `/healthz` returned 200 and the expected healthy response.
- `/history?ticker_name=%2A%2A%2A` returned 500; current source requires the existing 422 validation response.
- `/markets/not-a-market` returned 404; current source contains the market route and requires a 422 unsupported-market response.

An independent read of `/openapi.json` confirmed that the staging image publishes only `/`, `/healthz`, `/history` and `/info`. [Route inventory and image metadata](ASTRA-M08-YFINANCE-BASELINE-INVENTORY.json) record the bounded read. The registry confirms Linux AMD64 but contains neither an OCI source-revision label nor a source-repository label for this baseline. Its exact source commit remains unproven; do not assign it the current repository revision.

The current source contains both previously completed market/validation features. The M07 build from `efa6294954b01a089a65419c85542b8fc2f95c83` passed 37 tests and produced `sha256:33f1a08b74c82fb5dc01ef0ebef8a1fa5e2fc0ac78be17dadd1f74bbf1e319ca`. That build is not deployed and is not M11 acceptance. Its [existing build evidence](ASTRA-M07-YFINANCE-BUILD-VERIFIED.json) must remain distinct from the running baseline.

The frontend staging HTTP checks passed, but they only establish the served shell, mounted configuration and backend health proxy. They do not show that the current frontend JavaScript loads runtime configuration; the M11 change remains necessary.

The practical implication is straightforward: source completion and healthy infrastructure have overstated what can be inferred about the running application. M02's platform/readiness evidence remains useful; the current backend baseline does not meet the current-source functional contract. M08 must preserve this failed evidence, establish explicit baseline expectations and prove a real candidate deployment. M09 cannot call this a fully verified rollback target without its own recorded compatibility and health evidence.

No application patch, staging image update, production negative probe or production mutation was performed to obtain or repair these results. Routine advancement and the eventual Finance application changes must still be performed by PHarness. These observations cannot count as either M11 maintenance acceptance.
