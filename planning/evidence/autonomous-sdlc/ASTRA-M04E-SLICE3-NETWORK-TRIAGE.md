# ASTRA M04E Slice 3 — Test Diagnosis transport triage

Date: 2026-09-23. **Read-only live-cluster investigation; no model request, cluster mutation, or credential access.**

## Question and result

The exact-policy Test Diagnosis preflight `inferverify_01a0cfbc780674229db22b320941be10` was created at `2026-09-23T19:27:10Z` and completed failed at `2026-09-23T19:32:12Z` with sanitized failure `gateway protocol verification timed out`. It returned no calibration result. The target was `fireworks-nemotron-lightning-3p5-30b-a3b@v1`, through the `api.fireworks.ai:443` HTTPS proxy route, with a 300-second first-response timeout and three configured transport attempts. No Test Diagnosis qualification POST followed.

**Verdict:** the configured in-cluster egress path and allowlists were present and ready, but the observation does not prove the request reached Fireworks or establish where it stalled. Cilium/Hubble are not installed in this cluster, so there is no flow/drop evidence. The root cause remains unknown; do not retry the protocol preflight based on this evidence alone.

## Observed path during the event

- `pharness-model-gateway` was ready 1/1, and both proxy environment variables pointed to the in-cluster `pharness-coding-egress-proxy` Service on port 8080. The gateway pod and proxy pod were Running and Ready with zero restarts.
- The gateway NetworkPolicy allowed DNS to `kube-system/kube-dns` and TCP 8080 only to the selected coding egress proxy (plus its configured internal destination). It did not grant direct internet TCP 443 egress.
- The proxy Service had a ready EndpointSlice. Its NetworkPolicy allowed DNS and TCP 443 egress. Its application-level CONNECT allowlist included `api.fireworks.ai`.
- DNS Service and endpoint were present. No DNS result for the external provider was retained for this request, and no successful CONNECT or TLS handshake was logged.
- The live proxy's readiness probe was a TCP socket check every 5 seconds; its liveness probe was a TCP socket check every 10 seconds. Neither sends an HTTP CONNECT request.

## Log interpretation and limits

Proxy logs for the exact `19:27:10Z–19:32:12Z` operation interval contained repeated `proxy client closed before sending CONNECT` warnings at approximately 5-second cadence, with a second periodic series. Source code emits that error only when a TCP client closes before sending a request line. The two cadence patterns match the configured readiness/liveness TCP probes. This is strong evidence that these warnings are probe noise, but the proxy does not record the peer address or request correlation ID, so an individual close cannot be attributed conclusively to kubelet.

The proxy logs errors but not successful CONNECTs, and `pharness-model-gateway` currently logs process startup/configuration only, not per-request upstream phases. Thus these logs neither demonstrate an egress-policy denial nor show that the provider connection succeeded. No Cilium/Hubble agents, services, CRDs, or flow records were available. API/gateway readiness after the timeout only confirms process readiness, not model-provider reachability.

## Small signal-to-noise correction

The working tree now treats a clean close before any proxy request as a normal no-op and logs it at debug rather than warning level. This preserves optional diagnostics while preventing routine TCP probes from appearing as proxy failures. A focused async regression test exercises that close. It is **not built, deployed, or live-verified**; the currently served image remains unchanged.

Validation: `cargo test -p pharness-worker connect_proxy -- --nocapture` — 2 passed, 0 failed.

## Next slice

Before another Test Diagnosis attempt, add and release request-correlated, payload-safe transport diagnostics for the model gateway. At minimum the logs should distinguish outbound attempt start, connection/first-response outcome, and stream-idle termination, with elapsed time, target revision, and attempt number. Correlate to the existing preflight operation without logging bearer/model-grant tokens, credentials, prompts, full request/response bodies, or sensitive headers. Test timeout/error classification and prove secret/prompt redaction. After a compatible release is served and the diagnostic path is verified, re-read readiness and exact policy/target/registry hashes, then decide whether one new protocol preflight is justified. A failure remains terminal for that attempt; no qualification, Verifier, V2 activation, or connected WorkItem is authorized by this triage.

**Implementation update (2026-09-23):** request-correlated gateway phase logging and the proxy probe-noise correction now pass local regression tests. They have not been built or deployed; see [implementation evidence](ASTRA-M04E-SLICE3-TRANSPORT-OBSERVABILITY-IMPLEMENTATION.md). The release and live verification gate above remains open.
