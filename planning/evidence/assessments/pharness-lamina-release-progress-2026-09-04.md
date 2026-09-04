# Lamina release progress — 2026-09-04

> Historical checkpoint: this record captures the release before the local
> AMD64 emulation blocker was resolved. The completed build, GitOps rollout,
> and operational acceptance are recorded in the
> [final release result](../smoke-results/pharness-lamina-operator-console-release-2026-09-04.md).
> Its partial digests are not the deployed release artifacts.

## Requested change

Continue the approved Lamina operator-console milestone through its immutable
Mac-built release and disabled/enabled GitOps acceptance. **Not yet released.**
Source and deterministic acceptance are complete. The operator-approved cache
cleanup resolved the capacity issue, and six images plus the native bundle
are verified. The evaluator now fails
its actual Linux/AMD64 Rust execution check under QEMU. No release pins or
enabling flag have been merged; the current console remains the live default.

## Target and operation classification

- Repository: `lward27/pharness`; source
  `2d99156a410830aa0015995c779e6c3603fdab95`.
- Builder: explicitly selected Mac `rancher-desktop`, Linux/AMD64.
- Deployment target: context `lucas_engineering`, namespace `pharness`, Argo
  Application `argocd/pharness`, production Helm override unchanged.
- Source merges and publishing the seven PHarness artifacts are authorized by
  the approved plan. No broad Docker cleanup, host resize, cluster cleanup,
  PVC deletion, credential change, or registry configuration change occurred
  during this continuation.

## Repository conventions, plan, and files changed

Implementation, build, and release-pin worktrees are separate. The original
checkout and its untracked prototype, diagrams, screenshots, `temp.md`, and
other files remain untouched. The prototype still hashes to
`a24a8a442d08034d36165568f80cb141f6112d2b1f2fd2632a410e6664e9869c`.

1. [PR #319](https://github.com/lward27/pharness/pull/319) merged the Lamina
   implementation; merge `0c8caf12baa06dd649130866e42f00c7345fae8f`.
2. Live before-state inspection revealed that GitHub repository IDs are numeric,
   unlike the original fixture labels. [PR #320](https://github.com/lward27/pharness/pull/320)
   added a canonical-URL display helper, tests using numeric provider IDs, and
   four reviewed text-only compatibility screenshot updates. Durable IDs and
   mutation targets are unchanged. Its merge is the final source above.
3. The first source build was superseded before release. Its runtime image and
   interrupted UI publication are not accepted release artifacts.
4. Build all seven final images and the native bundle, then create a separate
   values-only digest pin; keep the redesign disabled through initial rollout.
5. Verify fallback, enable only the design flag through GitOps, then verify all
   eight routes, live Finance delivery/timeline records, and responsive behavior.

The implementation remains as described in the prior local acceptance record.
This progress update changes only planning documentation. The release-pin
worktree is still clean; no candidate digest or enabling flag has been merged.

## Validation results

| Gate | Result |
|---|---|
| Rust formatting | Passed |
| Workspace Rust tests | 620 passed across 97 reported test suites, zero failures |
| Workspace/all-target Clippy | Passed with warnings denied |
| UI production build | Passed |
| Vitest after label correction | 73 passed in 24 files |
| Final clean complete Playwright run | 103 passed; one existing duplicate mobile real-server invocation skipped |
| Separate real-server Lamina journey | Passed, including desktop and phone assertions; no browser API interception |
| Builder/revision checks | Passed; actual uncached Linux/AMD64 execution on the selected Mac builder |
| Helm/schema and namespace-aware Kubernetes dry-run | Passed before image pinning; repeat after pins exist |
| Rendered image scan | Existing image fields digest-pinned; final candidate scan outstanding |
| Paid models/Codex qualification | Not run; outside this UI milestone |

One intermediate browser run overlapped the real-server test ports and was
discarded. Both final browser invocations ran sequentially and passed. The
expected repository-name screenshot changes were reviewed; an unrelated
transient mobile glyph difference passed on recheck without changing a baseline.

## Verified partial artifacts

All six below were built from the final source on `rancher-desktop` and
verified for Linux/AMD64, OCI source/revision labels, and registry manifest digest.

| Component | Digest |
|---|---|
| Runtime | `sha256:fd1a8da53c991bc9b6ee003c895ebbb5b677d805802634d1de6fcc6fefb9f748` |
| UI | `sha256:9214ac8afb542aa846c4028629471dd8a0c0487a5241e49097dd088d8854a4eb` |
| Python runner | `sha256:e71ab55f5e4764755d51bbd9a51c91f2a451f2fe2e80f80dabba5c5163d642e6` |
| Node runner | `sha256:06b6b99cbd4d4cacd2a3eabb96e976b99f9dddde969320f1a8ca2a6f4b393c44` |
| Model gateway | `sha256:ba912c509994fd05a80bd4463634be6635fb8de819fe8fe0e6a7ecd02f03a5e6` |
| Codex host | `sha256:994598c7435c79b95178c4c6c660ae66d485c1f8a7faf1c1da45db0c6502e36c` |

The native Linux/AMD64 bundle passed its executable and bundled-file checksum
checks. Archive SHA-256:
`f3bc17114becdddffdf3cfed7d6ebb7552ce68a4d93f98de71c7177d8af6da9c`.
It has not been installed. Only the evaluator image remains outstanding.
This partial set must not be deployed as a completed release.

## Publishing workaround

Cloudflare returned HTTP 413 for a 105,529,124-byte Python/Codex layer. The
existing registry and authenticated write gateway were healthy. A temporary,
checksum-verified `regctl v0.11.6` client uploaded the exact Docker-exported OCI
content in 16 MiB chunks through the same public HTTPS registry and existing
Docker credential helper. No authentication boundary was removed.

The imported Python manifest exactly matched the original build digest. Later
runner builds generated distinct large layers, so cross-repository blob mounts
alone were insufficient. The publisher now exports each exact completed local
image, verifies its manifest and source/platform labels, uploads its own missing
large layers in chunks, and pushes the unchanged manifest without rebuilding.
The Node and Codex-host images passed that complete process. Registry/authentication configuration
is unchanged. Failed and superseded build logs remain separate from the exact
accepted component receipts.

The mechanism follows the official [chunked registry settings](https://regclient.org/cli/regctl/registry/set/)
and [OCI/Docker image import](https://regclient.org/cli/regctl/image/import/)
interfaces. The temporary Darwin/ARM64 client checksum was
`5b52b71861abc300ab513e87a4c3794cff6508035ac7f6b6bdfa1fa5688c0040`.

## Builder capacity issue and approved remediation

At 16:49 UTC, the Node image's package-index validation failed. Read-only
inspection found Rancher Desktop's Docker backing filesystem at **100% used,
zero available bytes**: 97.9 GiB total, 93.7 GiB used, with reserved space.
The Mac's host filesystem still had approximately 305 GiB available. Signature
verification was not disabled or bypassed; restore storage capacity first.

The operator explicitly approved clearing older cache. At 16:53 UTC, two
`rancher-desktop` BuildKit cache passes, both restricted to unused cache older
than 24 hours, reported 2.508 GB and 21.41 GB reclaimed. The second pass included
internal/frontend cache. No image, container, or volume pruning was performed.
The existing running container remained running. Docker storage then reported
20.4 GiB free, 78% used, and the same final-source build resumed.

The initial inventory-based estimate was conservative and not an exact measure
of reclaimable filesystem blocks; the cleanup receipts and post-cleanup free
space are authoritative. Removed cache is reproducible, not archived. Package
trust, the selected builder, VM size, registry configuration, and volumes remain
unchanged.

## Current blocker: Rust under AMD64 emulation

At 17:14 UTC, evaluator compilation succeeded, but its runtime check printed
`cargo 1.98.0` and then failed at `rustc --version` with
`qemu: uncaught target signal 11 (Segmentation fault)` and build exit 139.
A bounded exact-source retry reproduced the failure. The corresponding pinned
official Rust image also failed to return its version in a separate bounded,
network-disabled diagnostic run. That temporary diagnostic container was stopped
and automatically removed; no prior container was stopped.

Rancher Desktop reports VZ virtualization, Rosetta disabled, two CPUs, and 4 GiB
memory. Storage still had 13.3 GiB free after the failed retry. This is a distinct
execution/emulation blocker, not a justification to bypass signature checks,
remove runtime validation, change architecture, or deploy only six images.

The next proposed check is enabling Rosetta and retrying the exact evaluator
build. [Rancher Desktop documents Rosetta for x86_64 applications on Apple Silicon](https://docs.rancherdesktop.io/ui/preferences/virtual-machine/emulation/).
Its [settings command restarts the local backend](https://docs.rancherdesktop.io/references/rdctl-command-reference/).
The pre-existing `inspiring_wu` evaluator container has restart policy `no`, so
permission is required before interrupting that local Docker environment.
Rosetta is a candidate remedy, not yet a verified fix. No VM setting, memory
allocation, processor count, or unrelated container state was changed.

## Security and observability considerations

- No Secret values, credential files, auth headers, or session data are included
  in preserved evidence. Upload-query tokens are redacted from saved build logs.
- Agent-execution registry SHA-256 remains
  `011c83d5e4c1746da50ceee9d7b6c7a1057f740dfc99e60edcff7c4664fa7354`;
  inference-registry SHA-256 remains
  `6e0cf3931713c0b42e1f47a15223809a6fbda5623f91aa46806f90b092f90547`.
- The generic release-pin helper rewrites existing Codex policy runner hashes.
  This UI-only milestone must use a narrower reviewed values-only pin, preserving
  immutable historical/unqualified policy definitions. Do not promote policies.
- SBOM, signature, and provenance attestations are not verified. Digest and OCI
  label checks do not claim those stronger guarantees.
- Before-state evidence includes 12 browser views, six completed Finance flow
  responses, database generation, workload/image status, and Mimir CPU/memory/
  ingress observations. Browser navigation produced no mutations or errors.
- Public UI access returned HTTP 401 as configured; the public API hostname did
  not resolve from this client. Live acceptance uses the existing local cluster
  connection. No external-access configuration was changed.
- Idle ingress latency was unavailable, not zero. No throughput or performance
  improvement has been claimed.

## Rollout verification and rollback

The latest inspected Argo state was `Synced/Healthy/Succeeded` at final source
`2d99156a410830aa0015995c779e6c3603fdab95`, still running prior image source
`e9cda26431e0769d4025784128dd12ba18e426dc`. The five active PHarness Deployments
were each 1/1 ready. Database generation remains `dbgen_finance_20260827` with
schema `0049` and normal operational mode. No database migration is introduced.

New-image rollout, disabled/enabled verification, final observation window,
and operational acceptance are not yet performed. Keep this milestone active.
After successful rollout, rollback is the design flag set to false through
GitOps, without database rollback or evidence deletion.

## Evidence and destructive operations

Persistent local evidence root:
`/Users/wardl/.codex/artifacts/pharness/lamina-2026-09-04-2d99156`.
Its `manifest.json` records checksums, and `partial-artifacts.json` explicitly
marks the release incomplete. It includes sanitized test/build logs, before
screenshots/API snapshots, registry transfer observations, and the cache preview.
Temporary credentials/configuration, the downloaded CLI, and the image export
are excluded. Only the subsequently authorized older Docker build cache was
removed. No image, container, volume, cluster resource, database, or PVC was
deleted during this continuation.
