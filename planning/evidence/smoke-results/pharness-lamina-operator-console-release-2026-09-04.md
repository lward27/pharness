# PHarness Lamina operator-console release — 2026-09-04

## Result

**Accepted.** The Lamina operator console is enabled in the `lucas_engineering`
cluster at exact GitOps revision
`2d15c7c860233601f7715836c5ecd43a06179cad`. The deployed API and UI report
source `83a2689c877a3f48688d1d457c34e83474698c46` and remained aligned through
the final observation window.

The release preserves database generation `dbgen_finance_20260827`, schema
`0049`, normal operational mode, and the identities, statuses, and closure
timestamps of all 14 WorkItems. It introduces no database migration and no
execution, approval, delivery, or model-policy semantic change.

This record closes the
[approved milestone](../../implemented/milestones/pharness-lamina-operator-console-redesign-milestone-2026-09-04.md).
The [local acceptance record](../assessments/pharness-lamina-local-acceptance-2026-09-04.md)
and [release-progress checkpoint](../assessments/pharness-lamina-release-progress-2026-09-04.md)
remain the historical evidence for pre-release validation and resolved build
blockers.

## Source and GitOps chain

| Boundary | Exact identity |
| --- | --- |
| Initial implementation | [PR #319](https://github.com/lward27/pharness/pull/319), merge `0c8caf12baa06dd649130866e42f00c7345fae8f` |
| Live-data label correction | [PR #320](https://github.com/lward27/pharness/pull/320), merge `2d99156a410830aa0015995c779e6c3603fdab95` |
| Builder platform-proof correction | [PR #321](https://github.com/lward27/pharness/pull/321), merge and artifact source `83a2689c877a3f48688d1d457c34e83474698c46` |
| Seven-image digest pin, redesign disabled | [PR #322](https://github.com/lward27/pharness/pull/322), merge `d3ca9e13b0b64213a835018a5a384a5fea8fd3f7` |
| Redesign flag enable | [PR #323](https://github.com/lward27/pharness/pull/323), merge and accepted GitOps revision `2d15c7c860233601f7715836c5ecd43a06179cad` |

PR #321 changed the release preflight to accept a builder only after an actual
uncached Linux/AMD64 execution succeeds. It does not trust advertised BuildKit
platform metadata on its own.

## Immutable artifacts

All images were built from the exact source above on the explicitly selected
`rancher-desktop` builder. VZ Rosetta was enabled and an uncached Linux/AMD64
Rust execution passed before the final build. Remote manifests, architecture,
source/revision labels, and digests were checked after publishing.

| Component | Immutable digest |
| --- | --- |
| Runtime | `sha256:59530854356d2c8888f0e1715a2d5fab4d0a708bf1212b92a4fc50ed64d5c64d` |
| UI | `sha256:8945c2e59469fcbd3973e66e282e5c82c607bc9c5656fe154285962f9744c68e` |
| Python runner | `sha256:af8e076816452430d038c05cddbaeffd317a066afb613d4f9816fb91bc7413af` |
| Node runner | `sha256:41d471281c0aaddefb1ac58fa45e62aed015845dbc646c82d7d1f76baacc6d95` |
| Model gateway | `sha256:e696f3c1f573a5edfb3ffbb2dce799721d3bd20d3db9858a813e2caf56403672` |
| Evaluation runner | `sha256:6e838f21e22e4b864f4d45188ac92cb2bc6ac26b5bd206b18e472e2f84248dd8` |
| Codex host | `sha256:5583b6adbf5b8af8ac1912b608a102d62c900b8812bea554f7707592c07d45a1` |

The Linux/AMD64 native Codex-host bundle embeds Codex CLI `0.150.1` and has
archive SHA-256
`a71220f40b088a3789a90feecddaaaab28d6d20c7e4532f9fa62586e0099d0aa`.
Its internal executable and file checksum inventory passed. The bundle was not
installed as part of this milestone.

Cloudflare rejected several layers larger than 100 MiB with HTTP 413. The
already-built Python, Node, evaluator, and Codex-host images were therefore
published as the same locally verified manifests using 16 MiB chunked blob
uploads with checksum-verified `regctl v0.11.6`. No image was rebuilt during
that transfer and no registry or authentication boundary was weakened.

The checks above establish digest, platform, label, and source continuity.
They do **not** establish an SBOM, signature, or cryptographically verified
provenance attestation; all three remain unverified.

## Deterministic acceptance

| Gate | Result |
| --- | --- |
| Rust formatting | Passed |
| Workspace Rust tests | 620 passed across 97 reported suites; zero failures |
| Workspace/all-target Clippy | Passed with warnings denied |
| UI production build | Passed |
| Vitest | 73 passed in 24 files |
| Playwright compatibility | 103 passed; one intentionally skipped duplicate mobile real-server invocation |
| Real-server Lamina journey | Passed against temporary SQLite and real controller/store code without browser response interception |
| Helm lint and schema | Passed for disabled and enabled candidates |
| Server-side Kubernetes dry-run | All 53 rendered resources passed for both candidates |
| Manifest policy | No `:latest`; all PHarness workload images immutable |
| Registry configuration | Agent-execution and inference registry hashes unchanged |
| Scope guard | Coding Reliability V2 and Kubernetes Codex-host mode remained disabled |

The enabled candidate differed from the disabled candidate by exactly the
`features.repoModeV1.designOverhaulEnabled` boolean.

## Live rollout and observation

Target: Argo Application `argocd/pharness`, namespace `pharness`, context
`lucas_engineering`.

The digest-pin revision first reconciled with the redesign disabled. The
current-console fallback and all eight primary routes were then checked before
the flag-only revision was merged. The disabled verification found exact
source/image alignment, the preserved database generation and WorkItems, ready
endpoints, and zero restarts.

During the flag-only rollout, the API's `Recreate` strategy scaled the prior
ReplicaSet to zero at approximately 19:19 UTC. The replacement ReplicaSet was
not created immediately, leaving Argo `Progressing` for about five minutes and
forty seconds. Nodes, control plane, PVC, and desired Deployment state were
healthy, and there was no failed replacement Pod to diagnose. No direct patch,
restart, deletion, force sync, or rollback was performed. The Deployment
controller reconciled naturally and the new API became ready at 19:24 UTC.
This is recorded as a controller-reconciliation delay; the evidence does not
support a more specific root-cause claim.

At 19:24:57 UTC and again after the five-minute observation window at
19:29:38 UTC:

- Argo was exactly `Synced/Healthy/Succeeded` at
  `2d15c7c860233601f7715836c5ecd43a06179cad`.
- API, UI, gateway, and both egress proxies were ready with zero restarts.
- Live API, UI, and gateway image IDs matched their declared immutable digests.
- API/UI source revisions were aligned at `83a2689c...`.
- The inference registry was aligned and available.
- Database generation, schema, mode, and all 14 WorkItem states were unchanged.
- Each API, UI, and gateway Service had exactly one ready endpoint.

Mimir returned CPU, memory, throttling, and ingress series over the final
five-minute window. The system was idle and ingress rates were zero for the
sample; p95 request latency was unavailable (`NaN`), not zero. No performance
improvement is claimed.

## Live operator-console checks

The enabled console was inspected through the live Service connection, not the
fixture preview. Coverage included every top-level route, two completed Finance
WorkItems, desktop and phone widths, and the following assertions:

- The floating Lamina shell was present on all eight primary routes.
- The completed yfinance WorkItem showed six lifecycle lanes and the recorded
  Source Delivery interval including its external wait.
- Keyboard selection opened the interval inspector and Escape restored focus.
- The completed source delivery rendered `Source Delivery succeeded` with
  Release and Observe explicitly inapplicable.
- The legacy `0/5` delivery warning and reconciliation warning were absent.
- No page-level horizontal overflow occurred; phone content used one column.
- No browser mutation request or 5xx response occurred.
- Automated accessibility checks found no serious or critical findings on the
  exercised primary routes.

Visual review confirmed the intended glass shell, typography, hierarchy,
readable WorkItem ownership header, horizontal desktop laminae, and bounded
phone timeline viewport. Historical and current state remained distinct.

## Residual findings

These findings do not invalidate the UI milestone, but remain explicit:

- The flag-only API `Recreate` delay above should be characterized before a
  future rollout relies on a short API interruption budget.
- TCP liveness/readiness probes connect to each egress proxy and close without
  sending CONNECT. The proxies log those successful probe connections as
  warnings. Both proxies remained ready; the warning is noisy and pre-existing.
- Historical terminal Failed API Pods remain visible in the namespace. They
  were not running, mounted, or deleted as part of this release.
- Existing cert-manager Cloudflare DNS-challenge `PresentError` events were
  still present and are unrelated to the Lamina workload rollout.
- The retained local evaluator container remained stopped with restart policy
  `no`; it was not deleted.

## Evidence and rollback

The sanitized external evidence bundle is keyed
`lamina-2026-09-04-83a2689`. It contains build and publication receipts,
artifact identities, disabled/enabled render validation, rollout snapshots,
Mimir results, live browser observations, desktop/phone screenshots, and a
72-file SHA-256 inventory. Raw image exports, the large native archive,
registry configuration, credentials, authorization headers, and Secret values
are excluded.

Rollback changes only `features.repoModeV1.designOverhaulEnabled` to `false`
through GitOps. No database rollback or evidence deletion is required. The
prior console remains compiled as the flagged fallback.
