# ASTRA M04E Slice 3 — merged-source build stopped at BuildKit reachability

Date: 2026-09-23. This is a retained failed-build receipt, not image or release evidence.

## Source and review

PR [#404](https://github.com/lward27/pharness/pull/404) merged through GitHub at
`53f6909f2e7b59b3da845e6abb33c329e8d80c3c`, with parents
`cffa63f2cf41168883e798ded07d2bb9c2e65639` and
`15dab4783fb689b214d0027f1bbbce790e6846f4`. Its introduced-commit secret-scan
check passed. The focused local validation passed 15 model-gateway tests, 48
worker tests, and one hosted-build integration test. The merge did not change
GitOps or any running workload.

## Runtime build attempt

The selected `clone-build-push` Tekton Pipeline was dispatched as
`tekton-pipelines/pharness-runtime-53f6909` at 23:40:49 UTC, pinned to the exact
merge SHA. `fetch-source` succeeded. Its `build-push` TaskRun
`pharness-runtime-53f6909-build-push` failed at 23:42:10 UTC before BuildKit
listed workers:

```text
failed to list workers: Unavailable: connection error:
transport: Error while dialing: dial tcp 10.43.225.183:12340: i/o timeout
```

The PipelineRun targeted `registry.lucas.engineering/pharness-runtime:git-53f6909f2e7b59b3da845e6abb33c329e8d80c3c` with
`deploy/docker/Dockerfile.runtime`. The in-cluster `k3s-buildkit` Service maps
to the configured Mac endpoint `192.168.2.2:12340`; its EndpointSlice reported
that endpoint ready, but a bounded TCP connection attempt from this host also
failed. There is no NetworkPolicy in `tekton-pipelines`. The EndpointSlice
readiness is not proof that the remote BuildKit daemon is reachable.

No image digest/result was produced and no registry push completed. The
PipelineRun supplied an empty deployment parameter, so its rollout task was
skipped. No model-backed operation was dispatched, and Coding Reliability V2
remains disabled. The failed run and its PVC are retained as operational
history; no failed-build cleanup or speculative retry was attempted.

## Resume condition

Restore reachability of the owner-selected Mac BuildKit endpoint (or explicitly
select and validate a replacement), then rebuild both the runtime image (which
contains the worker) and `pharness-model-gateway` from the same merge SHA. Keep
the deployment parameter empty. Independently verify source labels, platform,
registry manifest digests, and runtime image IDs before considering any fresh
exact-policy Test Diagnosis preflight. Do not treat this failed run as a build,
release, or deployment.
