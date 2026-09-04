# Lamina local acceptance and release hold — 2026-09-04

## Requested change and verdict

Implement the approved Lamina operator-console plan, retaining existing resource
ownership, API mutations, approval controls, legacy delivery, and execution
semantics. Local implementation and deterministic acceptance are complete.
**The release is not complete.** Publishing, merge, release pinning, and Argo
rollout are held while the existing registry and PHarness API are unhealthy.

## Target and operation classification

- Repository: `https://github.com/lward27/pharness`.
- Clean implementation worktree: `pharness-lamina`, branch
  `codex/lamina-operator-console`.
- Base and rechecked remote `main`:
  `1770ad823a4970a8bc8a48d093f1cacc7c5bb001`.
- Authorized builder: Mac `rancher-desktop`, explicitly `linux/amd64`.
- Release target: Argo Application `argocd/pharness`, chart
  `deploy/helm/pharness`, Kubernetes context `lucas_engineering`.
- Local implementation/tests and sanitized cluster inspection/dry-run were
  performed. No source/GitOps PR, merge, image push, cluster mutation, pruning,
  or persistent-data operation was performed.

## Assumptions and repository conventions

The supplied prototype is a visual reference, not a new application runtime.
The existing React console, hash routes, Product model, server-derived actions,
and typed evidence remain authoritative. No migration or mutation endpoint was
added. The original checkout and its untracked diagrams, screenshots, prototype,
and other user files were preserved.

Prototype SHA-256, verified unchanged:
`a24a8a442d08034d36165568f80cb141f6112d2b1f2fd2632a410e6664e9869c`.

## Changes applied

- Added the disabled-by-default `features.repoModeV1.designOverhaulEnabled`
  Helm/API flag without changing `uiEnabled`.
- Added scoped floating navigation, glass surfaces, local IBM Plex fonts and
  licenses, patterns, dark/light tokens, phone navigation, reduced motion, and
  accessible dialogs. The prototype's generated runtime is not shipped.
- Added six recorded lifecycle lanes and a read-only interval inspector.
  Controller markers, repeated executions, repair lineage, missing timing,
  effective/historical outcomes, and source-delivery waits are distinct.
- Added the pure `lifecycle_timeline` flow projection. SourceDeliveryIntent
  timestamps preserve the PR wait that a closure-only StageExecution omits;
  the observation clock does not participate in action hashes.
- Preserved exact server action order and blockers. Removed historical-Run
  fallback, inferred stage completion, fabricated zero usage, and missing-head
  substitution. Stale confirmations require a fresh review.
- Shared the narrow organization request, guarded route/search/Repository
  selection races, retained explicitly stale data, and limited SSE to active
  Runs. Added paused-host and intentionally compacted-Run presentation.
- Restyled all eight sections, including Repository onboarding, scoped Finance
  topology, WorkItem sections, Run detail, Releases, Insights, and Settings.
- Made both release image building and native packaging require an explicitly
  selected normalized builder. Preserved exact source checks, registry targets,
  Linux/AMD64, immutable digests, and OCI source/revision verification.
- Added a noncached execution probe on the selected builder. There is no
  automatic alternate builder or architecture.

Primary implementation areas: `ui/src/repoMode`, `ui/src/views/RunDetailView.tsx`,
`crates/pharness-api/src/app/lifecycle_timeline.rs`, existing effective config,
the Helm chart, and the three existing build/package entrypoints. The screen
contract and `ui/AGENTS.md` now record the approved Lamina direction.

## Validation results

| Gate | Result |
|---|---|
| Rust formatting | Passed `cargo fmt --all -- --check` |
| Rust workspace | Passed `cargo test --workspace` |
| Rust lint | Passed `cargo clippy --workspace --all-targets -- -D warnings` |
| UI production build | Passed; both font licenses included in output |
| Vitest | 70 tests passed in 23 files |
| Complete Playwright compatibility suite | 103 passed; one intentionally skipped duplicate mobile real-server invocation |
| Lamina browser coverage | 18 tests across desktop/phone, including all primary sections, keyboard/focus, axe, light/tablet, reduced motion, stale actions, paused/compacted Runs, source closure and topology |
| Lamina real-server journey | Passed separately with `PHARNESS_UI_TEST_LAMINA=true`; real temporary SQLite/API/controller/provider-worker adapters, no browser API interception; desktop and phone completion assertions |
| Timeline API | Determinism, repair/repeats, missing timing, terminal missing-end records, intent wait, unchanged action hashes and read-only flow calls passed |
| Build scripts | Builder-selection, exact-revision, and native-package portability tests passed |
| Helm/schema | Lint/template passed; invalid nonboolean redesign flag rejected |
| Kubernetes dry-run | All 48 rendered resources passed server-side dry-run in their declared namespaces |
| Rendered image scan | Seven image fields, all digest pinned; no `:latest` |
| Actual Mac AMD64 execution | Selected `rancher-desktop` executed the uncached probe; `uname -m=x86_64`, 64-bit userspace |

The complete suite also retains the existing legacy real-server journey. Its
second mobile invocation is intentionally skipped by that existing test; phone
coverage is not skipped globally. The Lamina-specific real-server run is an
additional gate, not an intercepted fixture journey.

Existing visual baselines were changed only for reviewed correctness fixes
(unknown budgets, honest current state and labels), not to apply Lamina styling
to the fallback. New screenshot baselines cover the new design. No paid model
evaluation or unfinished Codex qualification was run or claimed.

## Preview and preserved visual evidence

`npm run preview:lamina` in `ui` serves a clearly nonproduction, GET-only Finance
fixture preview on `http://127.0.0.1:18442`. Its fixture API rejects writes; it
does not use the cluster API or subscription credentials. The five repositories
and six Services model Finance composition, but WorkItem IDs and timings are
presentation examples, not live acceptance claims.

Representative committed screenshots:

- [Recorded WorkItem lanes](../../../ui/tests/lamina-console.spec.mjs-snapshots/lamina-workitem-desktop-desktop-darwin.png)
- [Phone WorkItem](../../../ui/tests/lamina-console.spec.mjs-snapshots/lamina-workitem-mobile-mobile-darwin.png)
- [Onboarding](../../../ui/tests/lamina-console.spec.mjs-snapshots/lamina-onboarding-desktop-desktop-darwin.png)
- [Paused host, light tablet](../../../ui/tests/lamina-console.spec.mjs-snapshots/lamina-paused-light-tablet-desktop-darwin.png)
- [Compacted Run](../../../ui/tests/lamina-console.spec.mjs-snapshots/lamina-run-compacted-desktop-desktop-darwin.png)
- [Real-server completed journey](../../../ui/tests/repo-mode-real-server.spec.mjs-snapshots/lamina-real-completed-desktop-desktop-darwin.png)

## Rollout verification: observed blocker

Sanitized observations on 2026-09-04, approximately 14:30–15:00 UTC:

- `origin/main` and the live Argo synced revision both remain
  `1770ad823a4970a8bc8a48d093f1cacc7c5bb001`.
- Argo reports `Synced/Progressing`, **not Healthy**.
- Deployed image source revision remains
  `e9cda26431e0769d4025784128dd12ba18e426dc`.
- PHarness API readiness briefly recovered, then dropped again; the UI and
  gateway remained ready during the observation.
- `https://registry.lucas.engineering/v2/` returns HTTP 502. The registry
  Deployment has no ready replica during the failed checks.
- Recent registry Pods on `ubuntu-lucas-engineering` were evicted for low
  ephemeral storage. For example `docker-registry-c46cf8cd5-xdfkh` recorded
  threshold `9812356651`, available `7689380Ki`, container use `120Ki`, request 0.
- The node's DiskPressure condition had returned to False at 14:40:24 UTC,
  but the registry remained unavailable afterward. A cleared node condition
  alone is not evidence of registry or application recovery.
- Live API characterization requests for readiness, organization overview,
  Products and WorkItems through the UI port-forward returned 502. Accordingly,
  complete live before/after Finance evidence is still outstanding.

The recorded eviction is a concrete infrastructure fault. It does not establish
every cause of API instability; no speculative disk deletion or workload repair
was attempted. The `immutable-build-release` skill stops external effects on
failed readiness. The `k8s-workload-triage` skill bounded investigation to
sanitized workload status and storage-pressure evidence.

Existing pinned references observed (not new Lamina artifacts):

| Component | Digest |
|---|---|
| API runtime | `sha256:e21df7b298aa81d7787105129cc6ff1b84f79f87856c342005e3fc06b38bcd37` |
| UI | `sha256:ab8622f275e3f7a95068496e195ca5508398b541618809e51029da9a8ccc612a` |
| Gateway | `sha256:89fb89fffaebf67b846ecc1dd7f1fba7c1286067b45feda04ec91531de640f2f` |
| Evaluator | `sha256:b97330d542e7918f8d9f8b3fe11b17121823704806dfb04a1fb8ebc337718fc0` |
| Codex host | `sha256:db300957928317f5f1ae6b9392bdcb85a7d99284935fec1074378a2919292d4f` |

Database generation remains `dbgen_finance_20260827`, backed by
`pharness-api-data-finance-20260827`. No database/PVC contents were accessed or
changed for this redesign.

## Remaining plan and release gates

1. Separately diagnose/authorize remediation of the registry and API failure.
   Revalidate sustained registry access and PHarness readiness before effects.
2. Refresh `origin/main`, review/merge this implementation, and carry the exact
   resulting source SHA mechanically through release verification.
3. From a clean checkout of that SHA, build all seven components and the native
   bundle with `--builder rancher-desktop`. Do not reuse the current old image
   digests as evidence for this source.
4. Inspect every new artifact's Linux/AMD64 platform, source/revision, and digest;
   create a separate release-pin commit. Rerun Helm/schema, immutable manifest
   scan and namespace-correct server dry-run at that release revision.
5. Deploy through Argo with the redesign disabled. Require exact revision,
   image IDs, API/UI alignment, readiness and unchanged fallback behavior.
6. Enable only the redesign flag through GitOps; verify every primary route and
   actual Finance records and preserve their API/screenshot evidence.

There is no accepted Lamina image set, native bundle, release-pin revision,
enabled rollout, or live acceptance result yet. This plan stays active.

## Security and observability considerations

No credentials were read, emitted, archived, embedded in the preview, or moved
between hosts. No model/provider behavior or approval semantics changed.
Stage timing and usage remain distinct, unavailable data is explicit, and the
timeline cannot authorize or execute an action. Source merge remains manual.
Read-only inspection used explicit cluster/context/namespace targets.

The current build tooling explicitly reports SBOM, signature, and provenance
attestation verification as absent; a digest and OCI label are not those proofs.
No new claim about them is made here.

## Rollback procedure and destructive operations

No live change needs rollback. After a future accepted rollout, disable
`features.repoModeV1.designOverhaulEnabled` through a GitOps values commit;
retain `uiEnabled`, database generation, evidence, and execution state. Do not
down-migrate or delete data. **No destructive operation was performed.**
