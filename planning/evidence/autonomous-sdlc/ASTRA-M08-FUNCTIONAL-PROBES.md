# ASTRA M08: Deterministic Finance probes

Status: native probe implementation and automatic checks passed. The existing frontend staging baseline passed its limited HTTP checks. The existing yfinance staging baseline failed two application checks. No runtime window, autonomous deployment or production acceptance is claimed.

The implementation starts from deployment-reader commit `3fd06db3b825f6c6322a87990eafb515adac9963`. [Validation and source hashes](ASTRA-M08-FUNCTIONAL-PROBES-VALIDATION.json) bind the code, tests and live results. These changes are outside the sealed PHarness release at `19b0c55c48e3614d0b4507d56df3029a52475618` and are not deployed.

## Bounded behavior

The native method derives an in-cluster Service URL from the finite application/environment mapping. It accepts no arbitrary probe URL or path. All requests are concurrent GETs, with a maximum 15-second transport deadline and 64 KiB streamed response limit. Redirects, retries and ambient proxy routing are disabled. HTTPS retains normal certificate verification. Results retain status, elapsed time, response size and hash; response bodies, headers and raw client errors are omitted.

Backend probes check `/healthz`, rejection of a ticker containing invalid characters, and rejection of an unsupported market. The expected response contracts come from the current yfinance source, including its existing validation messages and 422 behavior. They do not require successful external market-data retrieval. Each backend request carries a recorded W3C trace identifier for later correlation; sending that identifier is not proof Tempo received it.

Frontend probes check the served application shell and exact non-secret environment configuration. Staging also checks its same-origin yfinance health proxy. They do not execute JavaScript or prove the application consumes the configuration; M11's runtime-configuration change and browser acceptance remain required. Frontend probes do not fabricate application trace evidence.

Unexpected application responses fail their check. Missing responses, timeouts and oversized bodies are inconclusive. Both stop a successful aggregate. Measured latency has no invented SLO; the transport deadline bounds the probe itself. Deployment identity and the full runtime window remain separate requirements.

## Validation

All 184 core unit/integration checks passed, including five new probe checks. The normal suite ignores three explicitly live-access tests. Tests cover validation status and shape changes, wrong environment configuration, transport loss, redirects, invalid bodies, declared and streamed oversize responses, body deadlines, trace headers and the absence of frontend trace claims. Clippy with warnings denied, formatting, dependency checks and the five dependency-parser tests passed.

Separate live reads used explicit loopback forwarding to the two staging Services in `lucas_engineering`. The requests exercised the same probe collection code; this verifies the HTTP contracts and responses, not the deployed API's in-cluster DNS or permission configuration.

| Existing staging application | Result | Evidence |
| --- | --- | --- |
| yfinance | Health 200 passed; invalid ticker returned 500 and unsupported market returned 404, both failed | [Backend results](ASTRA-M08-YFINANCE-FUNCTIONAL-PROBES-LIVE.json) |
| Frontend | Shell, staging configuration and staging yfinance health proxy passed | [Frontend results](ASTRA-M08-FRONTEND-FUNCTIONAL-PROBES-LIVE.json) |

The backend result is a real deployment/source discrepancy, described in [the baseline addendum](ASTRA-M08-FINANCE-BASELINE-DRIFT.md). It is not a reason to change the expected 422 response to 500 or accept a missing market route. The failed request was not repeated against production.

## Integration and recovery

Use these probes with the exact deployment reader, persisted query windows and application-scoped Mimir/Loki/Tempo evidence. Preserve observed baseline defects explicitly: a baseline observation and a candidate acceptance result answer different questions. Newly requested behavior cannot be assumed to exist in the preceding release. Define and record the baseline's supported checks before using it as a healthy automatic-rollback target; these failed candidate-contract probes cannot establish that target.

This slice does not connect the controller, admit a GitOps mutation, change an application or alter production authority. It adds no migration. Deploy through the full immutable PHarness release procedure and retain the current compatible database-reader floor. Source remains fixed while the separate corrected-runtime artifact set is built.
