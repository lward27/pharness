# ASTRA M08: Prepare the API to observe Finance

Status: implemented and checked at source `9c3a015a1e43de41e0ffa99eb378e5c07843168b`; not merged or deployed. Six in-cluster routing checks passed. The API still needs the compatible reader release and observed effective permissions before this gap closes.

## Why the change is necessary

The [live preflight](ASTRA-M08-API-RUNTIME-READER-PREFLIGHT.json) found that the API ServiceAccount could not perform the new Finance deployment-identity reads. Local administrator-wrapper checks could not demonstrate otherwise. The existing broad worker observer also lacked EndpointSlice access and is a separate model-worker identity; borrowing it would enlarge API authority unnecessarily.

The chart now prepares three namespace Roles and three bindings for `pharness/pharness-api`. In `apps-staging` and `apps`, it permits GET for the two named Finance Deployments and Services, and LIST for Pods, ReplicaSets and EndpointSlices. In `argocd`, it permits GET for the four exact Finance Applications. It grants no secrets, pod logs, exec, port-forward, watch, writes or cluster-wide list. Kubernetes cannot enforce a label selector on LIST: those three list privileges cover their named namespaces; the native reader's fixed selectors and owner-UID validation provide the narrower application check. These permissions are available before hosted creation, so compatible read access can be verified before autonomous writes are enabled.

Finance windows use a separate `cluster.finance_mimir_url` / `PHARNESS_FINANCE_MIMIR_URL`, configured as `http://mimir-gateway.monitoring.svc.cluster.local:80/prometheus`. Missing Mimir configuration stays inconclusive and never falls back to the Prometheus inventory endpoint. Existing source-only inventory behavior is preserved. The API receives the existing Tempo endpoint at port 3200. This does not add a coding backend, agent tool, worker permission, deployment mutation, workflow activation or execution-budget increase.

## Validation and actual routing

[Validation](ASTRA-M08-FINANCE-OBSERVER-VALIDATION.json) records 200 passing core tests and 17 configuration tests, plus Clippy across core/config/API targets, formatting and architecture checks. Tests verify file/environment precedence, explicit disabling and the absence of fallback requests to legacy Prometheus. [Chart checks](ASTRA-M08-FINANCE-OBSERVER-CHART-VALIDATION.json) verify the complete finite permission set, unchanged activation gates, expected telemetry configuration, rejection of an unrelated Mimir endpoint and server dry-run for all six RBAC objects. Dry-run did not apply them.

The first [routing check](ASTRA-M08-FINANCE-ROUTING-OBSERVED.json) reached every service, but its manually selected `up` query used nonexistent application scrape labels and correctly returned an empty, inconclusive result. That failure is retained. The [corrected check](ASTRA-M08-FINANCE-ROUTING-R2-OBSERVED.json) used the native reader's actual Pod-UID readiness metric and application request-counter labels; both Mimir checks, Loki, Tempo, backend health and frontend configuration passed at 23:23:34 UTC on 2026-09-05.

Both checks used a separately bounded non-root Job in `pharness`, with `pharness-api` as its ServiceAccount and no mounted ServiceAccount token, model credentials, repository credentials or volumes. The exact existing Python-runner digest is recorded in each manifest and observed Pod. This establishes in-cluster HTTP routing and response presence. It does not prove the current API binary ran these native checks, that new RBAC was applied, a fresh complete runtime window or application acceptance. No production request or application deployment occurred.

## Deployment, recovery and remaining gates

Hold this stacked change until the sealed source-19b0c55 release and live schema/history checks finish. Merge the native readers and this configuration together into the next compatible source release; keep hosted creation and Coding Reliability V2 disabled until their gates pass. After GitOps applies the Roles, verify effective access for each exact target and denial for unrelated namespaces, named workloads, secrets and writes. Then run native checks through the deployed controller path and record the live image/revision and persisted evidence.

The code adds no migration. Recover only with a reader compatible with the current schema-54 floor, preserving existing Finance history. Read-only permissions can be reverted in GitOps without authorizing a delivery rollback. M08 still requires durable baseline admission, candidate observation and real automatic staging; M09 production approval and M11 application changes remain open.
