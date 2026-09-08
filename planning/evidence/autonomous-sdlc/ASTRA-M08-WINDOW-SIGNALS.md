# ASTRA M08: Evidence across a real observation window

Status: implemented at `8173a4e83af22f1c2e178936e190644396f81ddc` and checked against both staging applications. Not deployed in PHarness or connected to durable progression. [Validation](ASTRA-M08-WINDOW-SIGNALS-VALIDATION.json) records the exact source and results.

The native reader collects fixed Mimir and Loki queries over a full five- or ten-minute window, bracketed by independently verified deployment observations. It rejects changed GitOps revisions, templates, Pod identities, restart counts, routing or image digests. It accepts no arbitrary query and adds no agent tool, execution authority, migration or configuration change.

## What the evidence establishes

Pod readiness, restart counters and metric freshness cover every expected Pod and 30-second sample. Loki queries bind namespace, container, Pod name and the Pod UID embedded in the log-file path. They require log presence in every sampled interval; an empty error result is meaningful only when that independent presence check succeeds. Counts, queries, scope and response hashes are retained. Raw log lines are not stored.

Backend HTTP counters cover the finite application/namespace job across all request addresses. [Live label inspection](ASTRA-M08-MIMIR-REQUEST-SCOPE-INVENTORY.json) showed that filtering by Pod-address `http_host` would exclude proxy/ingress traffic and that these counters have no Pod UID label. The evidence states this application-level correlation limit. Positive traffic with an absent status classification is inconclusive; an observed zero increment is retained explicitly, alongside required positive classified traffic. Frontend request metrics are not invented.

Missing, stale, nonfinite, partial, oversized, redirected or incorrectly scoped data is inconclusive. A readiness loss, restart change, HTTP 5xx or known log-error pattern is an observed anomaly. Neither means a confirmed release-caused regression or authorizes rollback. Even a fully observed signal window retains `runtime_verification: not_evaluated` until functional, identity, trace and delivery evidence are combined.

Each query has a 15-second maximum and a 256 KiB streamed-response ceiling, with no redirect or retry. There are seven backend queries and five frontend queries. Collection must finish within 60 seconds of window end. Counters are extrapolated by the metrics provider, and the output says so; no latency SLO is invented.

## What live testing exposed

The first five-minute windows were inconclusive. Loki aligned range evaluations to its 30-second grid, producing times outside the requested samples. The correction schedules the entire future observation window on that grid; it does not round a completed window or shorten the duration. Integer timestamps use Loki's documented nanosecond input convention. See the [Loki HTTP API](https://grafana.com/docs/loki/latest/reference/loki-http-api/).

The second aligned windows also remained inconclusive, this time because logs stopped arriving while both applications remained healthy and continued writing. [Collector evidence](ASTRA-M08-PROMTAIL-DELIVERY-GAP-PREFLIGHT.json) established repeated `OOMKilled` termination at 128 MiB. [GitOps PR 55](https://github.com/lward27/lucas_engineering/pull/55) changed only the collector's memory reservation and ceiling. Its [rollout](ASTRA-M08-PROMTAIL-MEMORY-ROLLOUT.json) and [post-window collector observation](ASTRA-M08-PROMTAIL-MEMORY-WINDOW-OBSERVATION.json) are recorded separately. Both collectors retained zero restarts; observed memory was 153 MiB and 89 MiB, below the 512 MiB ceiling. Delayed logs do not retroactively turn the failed runs into passes.

The third [backend](ASTRA-M08-YFINANCE-WINDOW-SIGNALS-LIVE-R3.json) and [frontend](ASTRA-M08-FRONTEND-WINDOW-SIGNALS-LIVE-R3.json) windows contain complete fresh samples and matching native deployment observations. They ran on the existing baseline images. A bounded historical query also found three matching log lines from the earlier failed backend probe; these are log matches, not a count of independent failed requests. [That check](ASTRA-M08-YFINANCE-KNOWN-ERROR-QUERY-CHECK.json) validates the known-error pattern without injecting another failure or claiming a fresh acceptance window.

## Engineering checks and remaining gates

All 192 core unit/integration checks passed, along with Clippy with warnings denied, formatting and architecture checks. Eight signal-focused checks cover wrong identity/time, syntax injection, missing samples, stale data, readiness/restart changes, wrong log UID, unknown/nonfinite counters, credential-bearing endpoints, malformed responses, redirects and response-size limits. Live-access tests remain explicitly gated. A stale test fixture and an incompatible standard-library convenience method were caught during development and corrected; their failed check logs remain available.

[Backend functional failures](ASTRA-M08-FUNCTIONAL-PROBES.md) remain unresolved for the pinned baseline. Frontend browser initialization remains M11 work. [Historical trace inspection](ASTRA-M08-BACKEND-PROBE-TRACE-IDENTITY-INVENTORY.json) confirms that the exact health-probe trace is retrievable, but its current attributes contain no Pod or image identity. Native trace correlation and durable integration remain required. Persist the full baseline before admitting a staging GitOps update; keep the release open until actual runtime acceptance. No production action or automatic rollback was performed.

Deploy this source through the complete immutable PHarness release procedure after the separate `19b0c55` artifact set and release pin. Preserve the schema-54 rollback floor. M08, M09, M11 and M12 remain open.
