# ASTRA M04: Registry transport during the combined release build

Status: investigation evidence retained; the underlying intermittent cause is unresolved. The original source `d675159cb205d222616d253d311706be1889bb91` build completed successfully at 19:14:29 UTC, after 2,176 seconds. Its seven artifacts and native bundle are independently verified. No infrastructure change, credential change or replacement build was performed.

The private BuildKit log reported TLS handshake timeouts while initiating uploads. At 18:53 UTC it contained five rendered occurrences representing two distinct full-line hashes; repeated progress rendering means those counts must not be presented as five independent failures. Runtime, UI, Python, Node and model-gateway receipts subsequently appeared in the same original build. The [completed timing record](ASTRA-M04-D675159-BUILD-TIMING.json) retains six final rendered TLS warnings with three distinct full-line hashes. Live release acceptance remains a separate gate.

## Observations

- The [bounded registry snapshot](ASTRA-M04-D675159-REGISTRY-TRANSPORT-OBSERVATION.json) found the backend and both write-gateway Pods ready with zero restarts. Neither gateway's retained 15-minute log sample contained a matched error. Backend blob-not-found checks are not automatically upload failures. This sample does not establish the absence of an intermittent network or gateway defect.
- Three direct Mac `/v2/` requests passed HTTP200 using the configured private CA and hostname verification. These alone did not test the builder's network namespace.
- [Actual builder route inspection](ASTRA-M04-D675159-BUILDER-REGISTRY-PROBE.json) verified the expected existing translation from registry hostname TCP443 to private NodePort32443 and three successful TLS handshakes. That first probe closed stdin before retaining an HTTP response; its null status is not an HTTP success.
- The [corrected read-only probe](ASTRA-M04-D675159-BUILDER-REGISTRY-PROBE-R2.json) kept the TLS connection open for the response. All three requests returned HTTP200 with CA and hostname verification, in 69–106ms. The original limited probe remains unchanged. No disabled TLS verification or authenticated write was used.

The [later registry snapshot](ASTRA-M04-D675159-REGISTRY-TRANSPORT-OBSERVATION-R2.json) captured one write-gateway timeout at 19:04:41 UTC. [Classification](ASTRA-M04-D675159-REGISTRY-TIMEOUT-CLASSIFICATION.json) identifies a POST for `pharness-eval-runner` that timed out **while connecting** from the gateway on `ubuntu-lucas-engineering-2` to registry Service `10.43.146.139:5000`. The evaluator subsequently published through the original build. This is a confirmed internal upstream connection failure, distinct from the client-side TLS warnings; the evidence does not establish a shared cause.

[Twenty subsequent read-only upstream probes](ASTRA-M04-D675159-REGISTRY-UPSTREAM-PROBES.json) all returned HTTP200: five fresh connections to the Service and five to its exact ready Pod from each existing gateway replica. Both local-node and cross-node paths worked at that later time. No NetworkPolicy, Pod placement, resource limit, gateway, registry or builder setting was changed.

A later successful probe cannot explain an earlier timeout. The previous [sourcee508 upload interruption](ASTRA-M04-REGISTRY-UPLOAD-RECOVERY.md) therefore remains relevant. A complete build that succeeds through its existing retry behavior is valid artifact evidence, but does not prove that the transport defect is repaired.

Full build stderr remains private outside Git because upload URLs can carry temporary authorization. Public evidence retains structured categories and hashes, not upload URLs, query values or credentials. No Finance or PHarness production deployment is implied by these transport checks.
