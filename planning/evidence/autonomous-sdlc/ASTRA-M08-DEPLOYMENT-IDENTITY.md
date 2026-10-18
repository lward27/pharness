# ASTRA M08: Native Finance deployment identity

Status: implemented and locally validated, including read-only observations of both existing staging applications. Not deployed in PHarness and not runtime acceptance.

This slice begins at PHarness source `19b0c55c48e3614d0b4507d56df3029a52475618`. Its changed-source hashes, test results and live receipts are in [the validation record](ASTRA-M08-DEPLOYMENT-IDENTITY-VALIDATION.json). It is independent of the immutable release being built from that source; it does not change that sealed release.

## What the check establishes

The native reader accepts only the two Finance applications, staging or production, a full expected GitOps commit and an immutable image digest. Application names, namespaces, GitOps paths, registry repositories and service ports come from the finite Lucas Engineering mapping. It introduces no agent tool, deployment action or production authority.

Six concurrent, read-only Kubernetes requests inspect the Argo application, Deployment, ReplicaSets, Pods, Service and EndpointSlices. Each request has a maximum 15-second deadline and 512 KiB response; lists over 32 items and incomplete pagination are inconclusive. Output is bounded while reading the child process, rather than after buffering an arbitrary response. Process failures and raw resource bodies are not returned. Evidence contains identity fields and response hashes, with no environment values, credentials or raw error text.

The expected GitOps revision, Argo source and destination, synchronized Deployment resource, fully observed rollout generation, current ReplicaSet template, owned ready Pods, running image identities and ready service endpoints must agree. A later Argo revision is inconclusive until the caller establishes a new authorized expectation. The reader does not infer ancestry or silently accept a different revision.

Read receipts are not an atomic Kubernetes transaction. The result explicitly records that limitation. A successful identity check does not establish a stable observation window, functional correctness, fresh application telemetry, regression absence, production approval or WorkItem success. `runtime_verification` remains `not_evaluated`.

## Validation and observed behavior

- All 179 core unit/integration checks passed; the two live-access tests remain ignored in the normal suite. The ten new automatic checks include malformed or missing evidence, stale GitOps revisions, wrong targets, incomplete rollouts, changed templates, incorrect Pod owners or digests, service misrouting, process failure, oversized output and deadline exhaustion.
- Clippy with warnings denied, formatting, the dependency check and its five parser tests passed.
- Separate authorized live reads passed for [yfinance staging](ASTRA-M08-YFINANCE-DEPLOYMENT-IDENTITY-LIVE.json) and [frontend staging](ASTRA-M08-FRONTEND-DEPLOYMENT-IDENTITY-LIVE.json), against GitOps `491f081e3ea6e639528a98cce43466cc7858fdcc`. These are the existing M02 baseline images, not new application deliveries.

Both Services retained an unready endpoint for a completed diagnostic Job sharing the application's label. Those Jobs are preserved. The reader excludes unready endpoints and counts only current Deployment-owned Pods. A ready endpoint to such a Job is rejected; a specific regression test proves that distinction. No resource was deleted, relabeled or restarted to make the check pass.

## Remaining work

Connect this reader to persisted staging reconciliation, collect the required five-minute baseline before admitting the GitOps write, and verify the candidate over the full staging window. Add bounded functional probes and application-scoped Mimir/Loki evidence, correlate instrumented backend Tempo evidence with the running release, and treat missing signals as inconclusive. Frontend runtime configuration and full delivery acceptance remain M11 work. Production observation is read-only; promotion and rollback still require the separate M09 contract.

This implementation has no migration or live configuration change. Deploy it only through the complete immutable PHarness release procedure. Preserve the schema-54 recovery floor established by the first compatible reader deployment; this read-only addition does not make earlier database readers compatible.
