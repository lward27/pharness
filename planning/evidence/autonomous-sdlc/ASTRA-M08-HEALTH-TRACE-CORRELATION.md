# ASTRA M08: Correlate the native health request with its trace

Status: implementation, core checks and one real five-minute staging trace check passed at source `7827b0698fac1e1df6538e9f0cc7c89c025acbb8`. The reader is not deployed or integrated into durable progression. Backend functional failures remain failures; M08 stays open.

## Result and boundary

The [live evidence](ASTRA-M08-HEALTH-TRACE-CORRELATION-LIVE.json) covers 2026-09-05 23:05:00–23:10:00 UTC, collected at 23:10:12. The exact native `/healthz` request returned 200 and its trace/parent identifiers matched one Tempo server span for `yfinance-wrapper` in `apps-staging`. Native Deployment, Pod imageID and Service endpoint observations bracketed the window at GitOps revision `9232b110177556b80e8a03197e17fc062965f4c3`, unchanged staging digest `sha256:f1cfc06fcac62d7c37a4d7dc87237e2abe02df0d9c3824a7521c5359058879c1`.

The invalid-ticker and unsupported-market probes returned 500 and 404. The complete functional result remains `failed`, and runtime verification remains `not_evaluated`. A health trace does not establish correct market behavior or turn this unproven-source baseline into a safe automatic rollback target. No application patch, new deployment or production action occurred.

## Implementation and evidence limits

The native reader derives and validates the trace and parent-span identifiers from the existing deterministic health-probe receipt. It reuses the deployment/window checks, makes one bounded Tempo V2 trace-by-ID request and requires a complete trace containing exactly one matching server span. It checks service/namespace, method, route, status, parent identity, span uniqueness and request/window timestamps, allowing at most one second of explicit clock skew. No raw attributes, events or trace body are retained; receipts record byte counts and a content hash. Limits are 15 seconds, 512 KiB, eight resource batches and 64 spans, without redirects or retries.

Missing, partial, oversized, malformed, unrelated, stale or failed-request evidence is inconclusive. Frontend results explicitly state `not_instrumented` and make no Tempo request. Trace payloads do not contain Pod or image identity in the observed application; the result records `trace_image_identity_verified: false` and cites the separate native Deployment/Service correlation. This is one request's evidence, not exhaustive tracing or regression causality.

The endpoint follows the [Tempo HTTP API](https://grafana.com/docs/tempo/latest/api_docs/). The live server reports Tempo 2.9.0; the [versioned response schema](https://github.com/grafana/tempo/blob/v2.9.0/pkg/tempopb/tempo.proto) defines complete/partial status, including the omitted protobuf JSON default for a complete response.

## Validation and remaining integration

[Validation](ASTRA-M08-HEALTH-TRACE-CORRELATION-VALIDATION.json) records 199 passing core tests, including seven new trace checks; five live tests stay opt-in. Clippy across core targets, formatting and architecture checks passed. Negative cases cover wrong parent/service/namespace/route/status, stale clocks, partial or duplicate spans, missing data, request failure, unsafe URLs, redirects and response limits. Initial Rust visibility and lint failures are retained; the shared validator's visibility was narrowed, without changing the evidence gate.

The [API deployment preflight](ASTRA-M08-API-RUNTIME-READER-PREFLIGHT.json) found no authorization for the new staging identity reads and no configured Tempo endpoint. Existing metrics configuration targets Prometheus, while the successful full-window tests queried Mimir. Local tests used the explicitly scoped operator wrapper and port forwards; they do not establish deployed API access. Finite API observer permissions, validated in-cluster telemetry routing, persisted baseline admission and controller integration remain required. M09 approval/recovery and M11 application acceptance remain separate gates.
