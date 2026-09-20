# ASTRA M04E: Connected-loop candidate release and dispatch readiness

Status: complete. The M04E connected-loop candidate runtime `49ecdc5` is deployed,
dispatch-ready, and Finance-preserving. This is not M04E connected-loop acceptance
and not M04 qualification; the first connected attempt (plan → implement → Test →
diagnosis → repair → Test → verification) remains the next slice.

## Scope and why the candidate moved

The slice plan (active/ASTRA-M04E-CONNECTED-LOOP-CANDIDATE-RELEASE-SLICE.md) targeted
a local build of `bc59f30` so that merged repair-authorization `67f5303` reached a
serving runtime. During execution, upstream main moved: PR #402 (WorkItem
prerequisite recovery) and PR #403 (pin WorkItem recovery release artifacts) merged,
and PR #403 pins a full seven-image release compiled from `49ecdc5`, which is a
strict descendant of both `bc59f30` and `67f5303`. Per the program rule (if
upstream moves, revalidate assumptions rather than replay stale changes), the slice
re-baselined onto the already-deployed `49ecdc5` instead of building a superseded
candidate.

## Local build attempts (retained history)

- Attempt 1 (worktree at `bc59f30`, builder `rancher-desktop`): revision
  verification refused — the main checkout is not clean (`tutorial-site/`
  untracked); a fresh detached worktree was created and used.
- Attempt 2 (fresh worktree at `bc59f30`): full Rust linux/amd64 cross-compile of
  `bc59f30` **passed** and the runtime image was assembled locally; the push to
  `registry.lucas.engineering` failed with 401 (this host had no push credential).
- Attempt 3 (after the owner supplied push auth via `docker login`): the build
  preflight refused because `origin/main` had moved past `bc59f30`; the candidate
  was superseded by the upstream `49ecdc5` release and no further local build was
  started. No partially pushed or relabelled artifacts exist for `bc59f30`.

## Candidate identities

| Boundary | Identity |
| --- | --- |
| Compiled source | `49ecdc5cc4eaf86197707dede6244bc55c888732` (descendant of `bc59f30`, includes `67f5303` repair-authorization) |
| Argo application | `pharness`, `Synced`/`Healthy`, source = pharness repo `deploy/helm/pharness` @ HEAD `83efe6a749bc6dbe153c456e8eca934e1e38b86c` |
| Runtime image | `pharness-runtime@sha256:4703d103ed30fd3d4fd9850a32187f59431c6ede4d2819b9f22b36c54042308c` |
| UI image | `pharness-ui@sha256:22e715c7f662d6eb8c6da335c4d750c7a721fa36aa4ebaeae57d9ce1d15905ac` |
| Model gateway | `pharness-model-gateway@sha256:89bb45c3c80b6103123845e72c46de928798f11b26aef48a47a472bda8cf6a63` |
| Eval runner / codex host / python / node | see [pinned image set](ASTRA-M04E-49ECDC5-SERVING-IDENTITIES.json) (all revision `49ecdc5`) |
| API/UI reported revision | both `49ecdc5`; `platform_versions_match: true` |
| Gateway registry hash | `sha256:a2850180384d782b9aa8fa0bc4aa811983c834281a0d263231ea4d3d18c49e3a` (API and gateway aligned) |

The `49ecdc5` rollout was performed by Argo auto-sync of the upstream pin commit;
no manual sync or rollout restart was issued. All three serving pods show zero
restarts.

## Dispatch readiness: 12/12 capabilities

All twelve isolated capability verifications were re-run against `49ecdc5`
(actor `lucas`, bounded isolated Jobs, no cluster mutation beyond the
verification records): `model_provider`, `source_workspace`, `source_reader`,
`source_writer`, `source_observer`, `gitops_writer`, `gitops_observer`, `tekton`,
`argo`, `observability`, `environment_profile:python-3.11`,
`environment_profile:node-24`. All returned `available`. See
[preflight bundle](ASTRA-M04E-49ECDC5-CAPABILITY-PREFLIGHTS.json).

## Ten-minute service window

[Window record](ASTRA-M04E-49ECDC5-SERVICE-WINDOW.json): 20 samples over
2026-09-20 18:24:02–18:33:33 UTC (600 s). All samples: `/health` 200 (avg 0.0235 s,
max 0.0282 s), API revision `49ecdc5`, registry hash `a2850180…`, single stable UI
content hash. Zero pod restarts; no new Warning events. These are internal PHarness
service checks, not Finance acceptance.

## Finance preservation

[DB preservation record](ASTRA-M04E-49ECDC5-DB-PRESERVATION.json). Read-only
copies of `pharness.db` (+WAL/SHM) were taken before (18:04 UTC) and after
(18:36 UTC) the service window. Per-table canonical-row SHA-256 set comparison:
**all 81 tables preserved, zero missing or rewritten rows**. 14 WorkItems, 82
Runs, 262 audit events, 4 retention holds, 102 stage executions/outcomes, 55
applied migrations, generation `dbgen_finance_20260827` unchanged. The only delta
is +12 rows in `capability_verifications`, written by this session's own
preflights. No SQL migration is in `49ecdc5` (migration inventory unchanged from
`291e007`).

## Flags and authority

- Hosted workflow: **disabled**; legacy WorkItem creation: **disabled**;
  Coding Reliability V2: **disabled**. No activation was performed or requested.
- No model, default, budget, limit, or suite change. The local GPT-OSS policy
  remains diagnostic-only; its failed Planner result is unchanged.
- No schema migration, no Finance production change, no connected-loop dispatch.

## Rollback floor

Preferred rollback runtime is the preceding observed release `291e007` (pin
`90ab524`). `49ecdc5` adds the WorkItem prerequisite-recovery read/API surface and
the capability-preflight audit logging; it introduces no migration, so data
compatibility is preserved. Because `291e007` and the older compatible-reader
floor `92f8f1b` predate the Planner readiness guard, **pause new Planner
work/activation before any rollback** to a release lacking that guard. Do not use
rollback to continue an expired or superseded chain.

## Acceptance (this slice)

- [x] One exact merged source; serving identities observed for API, UI, gateway
      and all seven pinned images; Argo `Synced/Healthy` without manual sync.
- [x] Full ten-minute service window passed with stable identities.
- [x] Live pre/post comparison: every Finance record preserved (schema 55, 14
      WorkItems, 82 Runs, audit and holds intact).
- [x] All twelve capability preflights valid against the new runtime identity.
- [x] Rollback floor recorded; hosted creation and Coding Reliability V2 remain
      disabled; no qualification is claimed.
- [x] Evidence committed under `planning/evidence/autonomous-sdlc/` with the
      `ASTRA-` prefix; M04 and the master updated from evidence.

## Limitations

- The `49ecdc5` build itself was produced and pinned upstream (PR #403) before
  this session; this record verifies the deployed result, not the upstream build
  pipeline. The local `bc59f30` compile proof is retained above as history only.
- Scratch build logs and DB snapshots live in a session workspace (pruned after
  72 h); the committed JSON bundles are the durable record.
- A 401 push from this host (no registry credential until the owner supplied
  `docker login`) is retained as transport history; it does not indicate a
  registry defect.
- M04E still requires the connected failure → diagnosis → repair → Test →
  verification proof; M04F still requires two full frozen qualifying runs on the
  final runtime. Neither is closed by this record.
