# ASTRA M08: Bounded native Finance trace collection

Status: implemented and locally validated; **not deployed and not M08 acceptance**.
Base source: `fb68b7fc6b0acf6dd8bf71fc4e13fd5c25ba2603`, isolated branch
`codex/astra-runtime-verification`, 2026-09-05. Hosted creation remains disabled.

## What changed

The existing `ReadOnlyClusterTools` now provides `observe_finance_traces` for
deterministic release verification. Its only target is `yfinance-wrapper` in
`apps-staging` or `apps-prod`. It does not add a model-facing action or a generic
query interface. Frontend traces are not invented.

The reader issues one GET to Tempo's search API. It fixes the service/namespace
query, caps results at twenty traces and retained spans at three per trace, and
requires an elapsed window no longer than ten minutes. An observation window
ending more than sixty seconds before the read is inconclusive. This bounds
collection freshness; it is not a service latency SLO. The eventual release
controller must still enforce the program's five-minute baseline/staging and
ten-minute production windows.

The complete request and streaming response share a maximum fifteen-second
deadline and a 512 KiB response ceiling; stricter configured limits take priority.
There are no retries, redirects, TLS exceptions, arbitrary queries, or error-body
logging. Credential-bearing endpoint URLs are rejected. The evidence retains
fixed scope, query/window, observation time, trace/span identity and duration,
newest matching span time, search job completeness and result limits. It also
records the raw response hash and byte count. Arbitrary span attributes, root
operation text, headers and raw trace bodies are not persisted.

`sample_available` means only that usable matching traces were returned.
`release_verification` stays `not_evaluated`, `deployment_correlation` stays
`not_established`, and absence of errors is never inferred. Missing, invalid,
unrelated, unavailable, oversized or stale evidence remains inconclusive.
Partial search metadata stays visible. Even completed search jobs do not make a
limited sample exhaustive: Tempo can stop after finding its requested result
count. [Tempo search API](https://grafana.com/docs/tempo/latest/api_docs/).

## Live finding and correction

The first local compiled read of `monitoring/tempo:3200` returned fresh staging
traces but rejected four identities containing thirty or thirty-one hexadecimal
characters. Tempo search can omit leading zeroes. The reader now normalizes
nonzero hexadecimal IDs to thirty-two lowercase characters before storing and
deduplicating them. Empty, nonhexadecimal, zero, oversized and duplicate
identities remain invalid. This matches Tempo's documented optional padding
behavior. [Tempo read configuration](https://github.com/grafana/tempo/blob/main/modules/overrides/config.go).

The corrected compiled reader collected **twenty matching staging traces** in
a five-minute window, with the newest returned span nineteen seconds old.
The response was 35,343 bytes and reported completed search jobs. The twenty-result
limit was reached; this is a sample, not an exhaustive error survey. No application
was changed or deployed. Both the initial inconclusive result and final bounded
projection are preserved in the [live observation](ASTRA-M08-TEMPO-LIVE-OBSERVATION.json).
The raw response hash is not a claim that the raw body has been retained.

## Validation and deployment boundary

- 180 core/configuration tests passed, with the live test excluded from that count.
- The explicitly enabled live read passed separately. Tests cover fixed requests,
  partial search data, real M02 response shape, malformed/missing/cross-environment
  data, freshness, invalid windows, normalized identities, duplicates, redirects,
  body size with and without Content-Length, timeout and diagnostic redaction.
- Architecture checks and their five parser tests passed. Formatting and diff
  checks passed; the source/log hashes and compiler checks are recorded in the
  [validation record](ASTRA-M08-TEMPO-VALIDATION.json).
- Default Helm output is unchanged across all fifty rendered resources. Explicit
  configuration renders the expected Tempo environment variable. The optional
  field is blank in defaults, so merging this preparation does not restart the
  current qualification runtime.

`cluster.tempo_url`, `PHARNESS_TEMPO_URL`, and Helm `api.cluster.tempoUrl` configure
the reader. The native cluster endpoint is
`http://tempo.monitoring.svc.cluster.local:3200`. API-to-worker configuration
propagation and a non-sensitive `tempo_configured` system indicator are included.
Enable the endpoint with the next compatible immutable runtime release, after
active qualification operations finish. No migration is required; no historical
record or hosted policy write changes in this slice.

M08 still needs automatic staging GitOps progression, observed Argo revision and
running image identity, functional probes, application-scoped metrics and logs,
trace/release correlation, and the complete failure/inconclusive/promotion tests.
This method is not yet called by the hosted release controller. There is no
claim of autonomous staging delivery or Finance end-to-end acceptance.
