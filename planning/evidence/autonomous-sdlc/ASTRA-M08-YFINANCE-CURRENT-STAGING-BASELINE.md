# ASTRA M08: Current-source yfinance staging baseline

Status: the program-operated staging refresh and one shared five-minute native observation passed. This is a validated starting point for later autonomous work, not the autonomous delivery or M11 acceptance result.

## Source, build and deployment

Application main remains `efa6294954b01a089a65419c85542b8fc2f95c83`. The [existing Tekton proof](ASTRA-M07-YFINANCE-BUILD-VERIFIED.json) connects that source, 37 passing application tests and immutable image `sha256:33f1a08b74c82fb5dc01ef0ebef8a1fa5e2fc0ac78be17dadd1f74bbf1e319ca`. [Fresh preflight](ASTRA-M08-YFINANCE-STAGING-BASELINE-PREFLIGHT.json) rechecked the current source and registry hashes/labels over verified TLS. No image was rebuilt and no application code was manually supplied.

[GitOps PR 56](https://github.com/lward27/lucas_engineering/pull/56) merged as `04a98931af43b6ea1d189369442f6a1b76dda589`, changing only the staging image digest. [Observed rollout](ASTRA-M08-YFINANCE-STAGING-BASELINE-ROLLOUT.json) records Argo Synced/Healthy, the exact running digest and new Pod UID `712b407b-d379-49f7-8400-0c4e67f12895`, with no restarts. The production Deployment's UID, generation and complete specification fingerprint remained unchanged. A normal Argo refresh requested a new comparison; its existing auto-sync performed the rollout.

## Shared observation window

The [combined baseline record](ASTRA-M08-YFINANCE-CURRENT-BASELINE-VERIFIED.json) covers 2026-09-05 23:34:00 UTC through 2026-09-05 23:39:00 UTC. The [signal reader](ASTRA-M08-YFINANCE-CURRENT-BASELINE-SIGNALS.json) and [health trace/probe reader](ASTRA-M08-YFINANCE-CURRENT-BASELINE-TRACE-AND-PROBES.json) used the same expected GitOps revision, digest and five-minute window at PHarness source `1b49cfd23f1e690a5b2b0e64f5978056b20c817b`. They each captured native identity before and after the window.

Health returned 200. Invalid ticker and unsupported market returned the required 422 responses, correcting the old staging image's observed 500/404 failures without changing the probes. Application request metrics, per-Pod readiness/restarts, fresh Pod-UID log streams and the exact native health request's server trace were observed. Missing-data, known-error and identity checks remained in force. Latency is recorded without an invented SLO, and the trace does not claim its own image attributes.

## What remains open

The old `f1cfc06` image's historical failures remain preserved in [baseline drift](ASTRA-M08-FINANCE-BASELINE-DRIFT.md). It is still the production image and cannot be treated as a proven current-source or healthy automatic rollback baseline. A production change requires a separate, concrete human approval before its GitOps merge.

These reads ran from the authorized operator environment, using the native code and bounded local port forwards; the hosted controller did not perform this refresh. M08 still requires persisted baseline admission and runtime progression through the deployed API, and a real autonomous staging demonstration. M11 still requires the two new maintenance changes, sequential pinned dependencies and genuine production approvals. No completed market feature or no-op has been counted toward those gates.
