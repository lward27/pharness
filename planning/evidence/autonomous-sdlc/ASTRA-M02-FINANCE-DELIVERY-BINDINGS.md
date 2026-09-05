# ASTRA M02: Finite Finance delivery bindings

Status: verified finite platform coordinates; runtime contract registration remains M05.
Observed 2026-09-05 UTC. GitOps revision
`137262f4377c5f1d19379f73e83249d66d09a0fd`.
All cluster operations are limited to `lucas_engineering`.

| Boundary | yfinance | Finance frontend |
| --- | --- | --- |
| Source repository | `https://github.com/lward27/yfinance_wrapper.git` | `https://github.com/lward27/finance-frontend.git` |
| Current source main | `12ff05dab47778dd2344970001c4218c1825db96` | `068510d3442efc75c036b327d3820b17c87c8c0a` |
| Product | `prod_01a043699d65721193e7e75d38654a2d` | Same Finance Product |
| Repository registration | `repo_01a04369f6387e6095aee4047ed2224f` | `repo_01a04369f1057e13a13c24233d4e278a` |
| Registry repository | `registry.lucas.engineering/yfinance_wrapper` | `registry.lucas.engineering/finance-frontend` |
| Tekton namespace | `tekton-pipelines` | `tekton-pipelines` |
| Pipeline | Existing `pharness-yfinance-build` | Reserved `pharness-finance-frontend-build`; actual Pipeline implementation M07 |
| Staging Argo application | `yfinance-staging` | `finance-frontend-staging` |
| Production Argo application | `yfinance-wrapper` | `finance-frontend` |
| Staging GitOps file | `charts/finance-staging/yfinance/kustomization.yaml` | `charts/finance-staging/frontend/kustomization.yaml` |
| Production GitOps file | `charts/yfinance-wrapper/kustomization.yaml` | `charts/finance-frontend/kustomization.yaml` |
| Workload/container | `Deployment/yfinance-wrapper`, container `yfinance-wrapper` | `Deployment/finance-frontend`, container `finance-frontend` |
| Staging namespace | `apps-staging` | `apps-staging` |
| Production namespace | `apps-prod` | `apps-prod` |
| Service / target port | `yfinance-wrapper:8090` / 8000 | `finance-frontend:8080` / 80 |
| Staging functional URL | `http://yfinance-wrapper.apps-staging.svc.cluster.local:8090/healthz` | `http://finance-frontend.apps-staging.svc.cluster.local:8080/runtime-config.json` |
| Production URL | `https://yfinance.lucas.engineering/healthz` | `https://finance.lucas.engineering/` |

The separately authorized GitOps repository is
`https://github.com/lward27/lucas_engineering.git`, branch `main`. Production and
staging use distinct paths and namespaces. All artifacts must be Linux/AMD64.
The image digest is determined by a verified real build and remains identical
through promotion. The application source SHA is not the GitOps SHA.

The recorded Product/Repository IDs originate in the accepted Finance generation.
The authenticated API confirmed generation `dbgen_finance_20260827`. Its current
PipelineContract and DeploymentContract inventories are empty; see
[contract inventory](ASTRA-M02-CONTRACT-INVENTORY.json). M05 creates declarations
through supported operations and records returned IDs before hosted writes.
The frontend production declaration requires the finite M05 contract change;
its old protected executor remains limited to yfinance. No IDs are fabricated here.

## Execution and signals

The existing remote BuildKit endpoint is
`k3s-buildkit.tekton-pipelines.svc.cluster.local:12340`, TLS server name
`buildkit-k3s.lucas.internal`. The existing Task references
`remote-buildkit-client-tls` and `lucas-registry-push`; their values must not enter
logs, parameters, or evidence. The API executor and the PipelineRun have separate
service-account boundaries. Record the actual identities from validated manifests
when dispatch is qualified. The existing yfinance Pipeline requires full source
SHA parameter `revision`, uses workspace `shared-data`, and returns
`SOURCE_COMMIT`, `IMAGE_URL` and `IMAGE_DIGEST`. The finite frontend Pipeline must
preserve that result contract. M07 verifies the actual merged source/build chain.

For yfinance, Mimir application data was observed under
`job="apps-prod/yfinance-wrapper"` with `http_server_duration_count`, duration
sum/buckets, and request labels. A `service_name` selector previously matched
inventory metrics and is not a valid substitute. A production query starting point
is `sum(rate(http_server_duration_count{job="apps-prod/yfinance-wrapper"}[5m]))`;
retain status-code and route detail for regression assessment. The live staging label is `job="apps-staging/yfinance-wrapper"`. M02 found three
request-count series and fresh samples in a five-minute window. Keep status/route
labels, and use `time() - timestamp(http_server_duration_count{job="apps-staging/yfinance-wrapper"})`
for observed freshness rather than confusing query evaluation time with sample time.

Loki queries use namespace, container and the exact released Pod. For example,
`{namespace="apps-staging",container="yfinance-wrapper",pod="yfinance-wrapper-66b99984d9-w7qq6"}`
matched actual application logs in M02. Select the corresponding current Pod at
verification time. Namespace/service alone also matched diagnostic probe logs.
Frontend Nginx logs are present under container `finance-frontend`; no frontend
application traces or application request metrics have been established.

Tempo returned real staging yfinance traces for
`{resource.service.name="yfinance-wrapper" && resource.service.namespace="apps-staging"}`.
Production already contains some error evidence. M08 must bind the query window,
release identity, expected behavior and regression thresholds; this discovery is
not a green release baseline. Do not invent frontend traces.
See [scoped M02 telemetry](ASTRA-M02-STAGING-TELEMETRY-VERIFIED.json).

## Accepted prerequisites and downstream gates

- Both exact baseline digests are available and running in staging. The backend's
  original artifact was restored without rebuilding; production remained untouched.
- Finance TLS validates; cert-manager 1.20.3 preserves all certificates/history.
- Separate application writer/observer and GitOps writer/observer capability checks
  passed. Their evidence is dated and must be refreshed before delivery effects.
- Service-routed Mac mTLS, uncached AMD64 execution, large private-registry push,
  and exact-digest pull/run passed. The Mac remains an availability dependency.
- All 13 required/denied network paths passed after the public-ingress correction.
  Relevant staging app signals are present; missing frontend signals remain explicit.
- Runtime contract IDs, complete source/build evidence, frontend runtime-config
  loading, autonomous progression and human production approvals remain M05–M11.

These are finite Lucas-specific bindings, not a provider-neutral adapter contract.
See [M02 execution evidence](ASTRA-M02-FINANCE-PLATFORM-READINESS.md) for deployed
identities and the exact boundary between observed, blocked, and planned behavior.
