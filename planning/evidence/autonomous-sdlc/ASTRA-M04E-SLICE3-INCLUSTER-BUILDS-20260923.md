# ASTRA M04E Slice 3 — in-cluster OCI build evidence

Date: 2026-09-23 (America/New_York); Tekton completion timestamps are 2026-09-24 UTC.
Status: both images built and pushed from the exact PHarness `main` head at dispatch (`72fe079`); **not deployed or released**.

## Source identity

- PHarness `main` and `origin/main` were reconciled to `72fe079d99cc8fa8ade7fe46cb30e6d234bb5604` before dispatch. A fresh `git ls-remote` immediately before the build returned the same revision.
- Both Tekton `fetch-source` tasks cloned that exact full SHA from `https://github.com/lward27/pharness.git`.
- This revision includes the Slice 3 transport diagnostics merged by PR [#404](https://github.com/lward27/pharness/pull/404) and the later mainline updates in PR [#405](https://github.com/lward27/pharness/pull/405). The previous failed build at `53f6909` remains retained separately in [the original stop record](ASTRA-M04E-SLICE3-RELEASE-BUILD-STOP.md); it was not rewritten or retried.
- Subsequent `main` commits in this task updated planning/evidence only; they changed no Dockerfile, Cargo source, lockfile or build context. The OCI source labels correctly remain bound to the exact `72fe079` tree built here.

## Builder and pipeline

- GitOps PR [#67](https://github.com/lward27/lucas_engineering/pull/67) installed the rootless BuildKit service, mTLS certificates, node-pinned cache, NetworkPolicy and build-only Tekton identity; squash merge: `5516197620c56be7fafe90cec35b90f2737ef3b6`.
- The first Argo rollout exposed a probe mismatch: BuildKit listens on mTLS TCP/12340 while its probes targeted an absent default Unix socket. PR [#68](https://github.com/lward27/lucas_engineering/pull/68) changed all three probes to the configured TCP listener and added a rendered-contract regression; squash merge: `2d1e485d5343aa6ef0afd24b494ff7ba6a2a572a`. The secret scan passed; 59 build-contract checks, Helm lint and server-side dry-run passed.
- Argo `tekton-ci` is `Synced/Healthy` at `2d1e485d5343aa6ef0afd24b494ff7ba6a2a572a`. The BuildKit Pod was Ready on `ubuntu-lucas-engineering-build`; its 60Gi `local-path` cache PV has node affinity to that node, its three certificates were Ready, and the Service EndpointSlice was Ready.
- Both runs used `clone-build-push` and service account `tekton-ci-build` (automount token disabled). Their `deployment` parameter was empty, and Tekton reported `rollout-restart` skipped. Compilation occurred on the in-cluster BuildKit node; the PipelineRun client Pods ran elsewhere.

## Immutable artifacts

| Image | PipelineRun / TaskRun | Source revision | TaskRun digest | Independently fetched registry manifest SHA-256 | OCI config |
| --- | --- | --- | --- | --- | --- |
| `registry.lucas.engineering/pharness-runtime:git-72fe079d99cc8fa8ade7fe46cb30e6d234bb5604` | `pharness-runtime-72fe079` / `pharness-runtime-72fe079-build-push` | `72fe079d99cc8fa8ade7fe46cb30e6d234bb5604` | `sha256:9719b5126d1cd74bd5652e11cfc2e1ad5413a2eb82ad33fbb5e863fcb72ce86a` | `sha256:9719b5126d1cd74bd5652e11cfc2e1ad5413a2eb82ad33fbb5e863fcb72ce86a` | `sha256:a24c166b1c6fc2322e24ae361f52c3cf4c2213428426da85ce62914b9e67b168` |
| `registry.lucas.engineering/pharness-model-gateway:git-72fe079d99cc8fa8ade7fe46cb30e6d234bb5604` | `pharness-model-gateway-72fe079` / `pharness-model-gateway-72fe079-build-push` | `72fe079d99cc8fa8ade7fe46cb30e6d234bb5604` | `sha256:db84a1d4a3f29634ad3c754f63d1ca13f86664fd363feea53ab14342a3236747` | `sha256:db84a1d4a3f29634ad3c754f63d1ca13f86664fd363feea53ab14342a3236747` | `sha256:73aacc95ba29ec716e5ecc70447cc60a8de0cec7608d79905ac7187f1b519baa` |

Independent registry verification used the Distribution API to fetch each manifest by its immutable Git tag, hash the returned manifest bytes, fetch its config blob, and inspect only architecture, OS and OCI labels. Both independently computed manifest hashes equal the Tekton `IMAGE_DIGEST`; both configs report `linux/amd64`, `org.opencontainers.image.source=https://github.com/lward27/pharness`, and `org.opencontainers.image.revision=72fe079d99cc8fa8ade7fe46cb30e6d234bb5604`.

## Release boundary and next action

At observation time, Argo `pharness` was `Synced/Healthy` at configuration revision `72fe079d99cc8fa8ade7fe46cb30e6d234bb5604`, while the live API and gateway remained on older pinned digests `sha256:4703d103ed30fd3d4fd9850a32187f59431c6ede4d2819b9f22b36c54042308c` and `sha256:89bb45c3c80b6103123845e72c46de928798f11b26aef48a47a472bda8cf6a63`. The new artifacts have not been pulled into or served by PHarness; no deployment pin, workload, model setting, evaluation, qualification or connected WorkItem was changed or started. Coding Reliability V2 remains disabled.

The next gate is a separately reviewed and explicitly approved immutable image-pin/release change, followed by rollout observation and verification of the actually served API/gateway identities and readiness. Only then may the exact-policy inputs be re-read and a new Test Diagnosis protocol preflight be considered under the active slice's stop rules. This build evidence does not authorize deployment or a model-backed operation.

## Source-drift reconciliation — 2026-09-24

The original build receipt above remains valid for its exact `72fe079` source and
digests, but it no longer identifies current PHarness application source. PR
[#414](https://github.com/lward27/pharness/pull/414) merged M06 Planner-startup
API changes in `af6db7dd4934847285795f7eda953436c972be95`. Current PHarness
`origin/main` is `3e8cbefec2a1894d9d53d35f2aa262b564292870`; the diff from
`72fe079` includes changes in nine `crates/pharness-api` source/test files.
Therefore the old runtime image is stale for a current-main release candidate,
even though its OCI digest and historical revision label are correct. The old
model-gateway artifact remains a valid `72fe079` build, but it must be rebuilt
alongside runtime from one shared current source SHA for a coherent candidate.

On 2026-09-24, initial server dry-runs passed for `runtime` and `model-gateway`
at `3e8cbef`. PR #417 later advanced main to `f6a6425` with planning-only
changes; no application build inputs changed. The preflights were repeated at
that main head and passed using `clone-build-push`, context `lucas_engineering`,
namespace `tekton-pipelines`, and `tekton-ci-build`. Their deterministic names
are `pharness-runtime-f6a6425fceb9` and
`pharness-model-gateway-f6a6425fceb9`; neither PipelineRun was created by these
dry-runs. Re-run preflight against the exact remote-main SHA immediately before
any later dispatch. The builder endpoint was Ready on
`ubuntu-lucas-engineering-build`, and Argo `tekton-ci` was Synced/Healthy at
Lucas main `9de1e2d236b87a1bdb96a0278ddfb7bddfd33a00`. A new registry push still
requires explicit authorization for these two image repositories. Any
subsequent GitOps pin/rollout remains a distinct, separately approved release
effect; no Test Diagnosis or other model operation is authorized by this note.
