# ASTRA M04E: Git-egress repair re-release and readiness restoration

Status: complete. The worker git-egress regression from `bc59f30` is repaired, the
runner images are re-released at `111fb9b`, and repository readiness for
`finance-frontend @ 28e55351…` is restored to `coding_status: ready`. This closes the
dispatch-path blocker found by the [M04E dispatch-path audit](ASTRA-M04E-DISPATCH-PATH-AUDIT.md);
the connected loop is again dispatchable (V2 policy re-qualification and the first
connected attempt remain the following slices).

## What was repaired

`crates/pharness-worker/src/main.rs` `repository_git_output` (hardened by `bc59f30` with
`.env_clear()`) dropped `HTTPS_PROXY`/`https_proxy`/`NO_PROXY`/`no_proxy`. Runner-preparation
egress is NetworkPolicy-restricted to the preparation egress proxy, so every source
`git fetch` through that helper failed. Fix (commit `111fb9b`): re-allow exactly those
four preparation proxy variables (only when set) in the git subprocess environment via a
new pure `repository_git_environment()` builder, preserving the `bc59f30` hardening
(hooks disabled, global/system config disabled, token via askpass only). Deterministic
unit test `repository_git_environment_allows_only_the_minimal_set_and_optional_proxy`
added; `cargo test -p pharness-worker` 47/47 + 1 integration pass;
`cargo check --workspace --all-targets` clean.

## Release identities

| Boundary | Identity |
| --- | --- |
| Compiled source | `111fb9bf651843a3d1a424e8111130ece2c48898` (fix commit; descendant of `49ecdc5`) |
| GitOps pin commit | `b62367f4429e16db3aab995bcbac016735ed4b1d` |
| Argo application | `pharness`, synced revision `b62367f4429e16db3aab995bcbac016735ed4b1d`, `Synced`/`Healthy` |
| node-runner | `registry.lucas.engineering/pharness-node-runner@sha256:3420493cb3856b99f7a6e2da377e0c928dfb2b53ed584123ca0e2d0f4682b409` |
| python-runner | `registry.lucas.engineering/pharness-python-runner@sha256:349f575cd8151856025055a179e9ede8f5f73c947ccdaaa683c35d6ff4b0ad16` |
| runtime (API) | unchanged `pharness-runtime@sha256:4703d103…` (`49ecdc5`) |
| UI | unchanged `pharness-ui@sha256:22e715c7…` (`49ecdc5`) |
| API/UI revision | both `49ecdc5cc4eaf86197707dede6244bc55c888732`, `platform_versions_match: true` |
| Gateway registry hash | `sha256:a2850180384d782b9aa8fa0bc4aa811983c834281a0d263231ea4d3d18c49e3a` |
| Env profiles | node-24 + python-3.11 revision `111fb9b…`, active |

Scope note: only the two environment-profile runner images (and the inert codex
`profiles:` block) were bumped. The runtime/UI stay at `49ecdc5` (version-lock gate
compares api↔ui only — there is no API↔runner cross-revision gate), so the control plane
was not re-imaged and `platform_versions_match` remains true. The
`agent-execution-registry.json` runner_images stay at `49ecdc5` by design: bumping them
recomputes `policy_hash`/`config_hash` and would invalidate the currently-passing
builder/repair V2 qualifications; that is the next slice's work.

## Build and push notes (homelab transport)

- The in-cluster Tekton `clone-build-push` path was unavailable: its BuildKit daemon is
  the owner's M1 Mac (`192.168.2.2:12340`), powered off (unreachable from the host and
  from inside the cluster).
- All four images were built locally (`scripts/pharness-build-local.sh`, builder
  `rancher-desktop`, clean worktree at the verified SHA, full uncached cross-compile).
- `runtime` (119 MB) and `ui` pushed over the public registry route. `node-runner`
  (359 MB) and `python-runner` (327 MB) hit the documented Cloudflare per-request body
  limit (HTTP 413) on that route — the known reason the private write gateway exists.
  Both were pushed from inside the cluster via the private write gateway
  (`registry-write-gateway.registry.svc.cluster.local:8080`, `client_max_body_size 0`)
  with `crane` and the `lucas-registry-push` credential, from a pod in the
  `ingress-nginx` namespace (the namespace the gateway NetworkPolicy allows).
- Registry artifacts verified: manifest digest matches the build output, linux/amd64,
  `org.opencontainers.image.revision=111fb9bf…`, source label `github.com/lward27/pharness`.

## Readiness restoration (acceptance)

Fresh capability preflights on the new node-runner: `source_reader` `available`,
`environment_profile:node-24` `available`. Fresh readiness assessment
`rready_01a0caea26517d428685fef692a5c9bb` for
`repo_01a04369f1057e13a13c24233d4e278a` @ `28e553510b1e0b82e3105c754a7019145829ab55`:

- `coding_status: ready`, `contract_status: ready`, `blockers: []`, `current: true`.
- checks: `exact_checkout: passed`, `canonical_contract: passed`,
  `environment_preparation: passed` (runner_image
  `pharness-node-runner@sha256:3420493c…`, profile revision `111fb9b…`),
  `declared_acceptance: executed`.
- Readiness-prep Job `pharness-repository-ready-5aa89cabdfe7`: 1/1 Complete in 57 s
  (2026-09-22 20:58:04→20:59:01 UTC). Two earlier runs (59 s, 57 s) completed identically;
  on `49ecdc5` the same step failed `git_fetch_failed` 3/3. The A/B probe from the audit
  (proxy-cleared fetch rc=128 vs proxy-inherited fetch rc=0) is the reference; this is the
  in-situ confirmation that the fixed worker reaches github.com through the egress proxy.

## Service window

[Window record](ASTRA-M04E-111FB9B-SERVICE-WINDOW.json): 20 samples over
2026-09-22 21:14:08–21:23:39 UTC (600 s). All samples `/health` 200 (avg 0.0245 s,
max 0.0309 s), API revision `49ecdc5`, registry hash `a2850180…`, single stable UI hash.
Zero pod restarts (api/model-gateway/ui).

## Finance data preservation

[Comparison record](ASTRA-M04E-111FB9B-DB-PRESERVATION.json).

- Service-window bracket (pre 21:09 UTC / post 21:26 UTC): **preserved=True** — 81 tables,
  0 missing, 0 rewritten.
- Pre-deploy baseline (04:05 UTC, `49ecdc5` runtime) → post-window: all deltas are
  additions from this session's readiness runs (+3 assessments, +3 preparations, +3
  workspaces, +6 capability_verifications) plus one `organizations` row whose only changed
  field is `updated_at` (1789910901301 → 1790050014645; id/key/display_name/created_at
  unchanged) — a no-content `ON CONFLICT … DO UPDATE` timestamp touch
  (`crates/pharness-store/src/sqlite/product.rs:19-22`), not a data rewrite. No Finance
  production rows, migration, or schema change. Generation `dbgen_finance_20260827`
  unchanged.

## Rollback floor

Rolling back = revert the pin commit `b62367f` (profiles return to the `49ecdc5` runner
digests); no data migration is involved and the `49ecdc5` runner images remain in the
registry. The `bc59f30` lineage is unchanged upstream. Rolling back to pre-`49ecdc5`
(`291e007`/`90ab524`) still requires pausing Planner work, per slice 1.

## Boundaries held

No connected-loop dispatch, no V2 flag change, no policy re-qualification, no NetworkPolicy
or egress-proxy change, no schema migration, no Finance production change. The two
remaining blockers from the dispatch-path audit (three unqualified V2 stage policies;
`PHARNESS_CODING_RELIABILITY_V2_ENABLED=false`) are unchanged and owned by the next slice.
