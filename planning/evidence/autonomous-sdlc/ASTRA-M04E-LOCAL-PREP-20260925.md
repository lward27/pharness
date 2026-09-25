# ASTRA M04E: local preparation run, 2026-09-25

This is the single evidence record for the 2026-09-25 local preparation run. It released current main (the M04E Slice 3 release step) and ran one Test Diagnosis exact-policy protocol preflight. That preflight **failed with a new failure class**, so the serial qualification chain stopped. The handoff is [LOCAL-PREP-HANDOFF-20260925](../../handoffs/LOCAL-PREP-HANDOFF-20260925.md). Full JSON receipts are stored privately on the operator machine under `pharness-release-artifacts/97c7e338630088ee8da70e0b7f554666dce8664b/`.

## Source identities

| Item | Value |
| --- | --- |
| pharness `origin/main` at start | `3eaa3129b72b8f21de3fe6713119f3bccdbb9dda` |
| fmt fix (#420) → built source | `97c7e338630088ee8da70e0b7f554666dce8664b` |
| release pin (#421) → Argo revision | `c3395e8907be16109a7cd9161498076de82d1a6a` |
| script fixes (#422) → main before this record | `a0d748b32ddfbf0f08833e842e100c464412f1af` |
| lucas_engineering `origin/main` (unchanged) | `9de1e2d236b87a1bdb96a0278ddfb7bddfd33a00` |

## Baseline before release (13:17–13:23 UTC)

- `lucas-ops doctor --credentials --builder` was `blocked` by one check, `buildkit_endpoint`. The profile still expects the retired Mac-route EndpointSlice `k3s-buildkit-ipv4`, but the live builder is the in-cluster `k3s-buildkit` Pod on `ubuntu-lucas-engineering-build`. Every other check passed: cluster identity, pipelines, registry TLS, all three credential references, PHarness authentication, source protection and GitOps authentication.
- Argo: every application was Synced/Healthy, including `pharness` (at `3eaa312`), `finance-frontend`, `yfinance-wrapper`, `finance-frontend-staging`, `yfinance-staging` and `finance-app-database-service`. The only exception is `sentinel-rbac`, whose sync status is Unknown.
- Served runtime was `49ecdc5` (runtime `sha256:4703d103…`, gateway `sha256:89bb45c3…`, UI `sha256:22e715c7…`). Pod imageIDs matched the Deployments, with 0 restarts.
- Readiness: mode `normal`, inference `available`, registry `sha256:a2850180384d782b9aa8fa0bc4aa811983c834281a0d263231ea4d3d18c49e3a` aligned on API and gateway. All ten capability verifications and both runner profiles were `stale` (their 900 s TTLs had lapsed).
- Data (API inventory; the live database was not read directly): 14 WorkItems, 82 Runs, 4 retention holds, and no active evaluation or `pharness` Job.
- Finance TLS certificates were Ready. `apps-staging` and `apps-prod` Deployments were all ready. Loki, Mimir (gateway) and Tempo `/ready` answered through the API-server service proxy.

## Validation at `3eaa312`

`cargo fmt --check` **failed**. The formatting drift in `pharness-worker` was fixed by #420. `cargo clippy -D warnings` passed. `cargo test --workspace` exited 0, with 875 passed (the 8 `hidden_semantics` lines are seeded fixtures). Both architecture scripts passed. UI: `vitest` 97/97 and `vite build` passed. Playwright: 136 passed and 1 skipped. The real-server spec first timed out in `beforeAll` while competing for the cargo lock, then passed when rerun alone. main's CI runs only the secret scan.

## Release of `97c7e33`

- Tekton `clone-build-push` on in-cluster BuildKit, PipelineRuns `pharness-<component>-97c7e3386300`, 13:38–14:28 UTC. The repository script needed two fixes before it would run on macOS jq and bash (see #422); the build used an out-of-tree copy carrying exactly those two edits.
- `lucas-ops release verify` returned `verified_artifacts`: every image is linux/amd64 and its OCI revision and source labels equal `97c7e33`.

| Component | Digest |
| --- | --- |
| runtime | `sha256:ae83610075a9f976f87ac4fb7b988c28a9570d8f8acb67125b3930898a9c8121` |
| ui | `sha256:9aee6a25d337e41ebf7a9c9e1c550cc71c617450d81557b791fc60acfbb9f4fa` |
| python-runner | `sha256:6335c271c8a841fd2ee7cd90ffe85fca16be3312dc0614d6d23ba20b2aa8b3d4` |
| node-runner | `sha256:1de54a7fea1126c43b1f72a3536d29eab36645306aec253fd093d8c0756f55e7` |
| model-gateway | `sha256:69289e1a7de9aeb6679afeb25d893f2c859fb0ca07d40e273b1bb448cf272da3` |
| eval-runner | `sha256:2953bccbaeb64f074d9770ea9b6f95f5259781262077a2c6b17aa9cfd3c588fb` |
| codex-host | `sha256:319e48299878712de6ec876a926aed2a1b368f373036637602fd9a4538b16ed5` |
| codex-host bundle image | `sha256:553c5744331c09af6b34dcf2480d1954237cee741a57144971bb1af970ee05f8` |
| native bundle tar.gz | `sha256:77ffa324facd33430dc678cd5bc33811c2cc847b5594a190fc7d7d252c09fe76` |

- Pre-release archive: `pharness-data-archive-pre-release-97c7e33-20260925`, written by the served `4703d103` runtime from `pharness-api-data-finance-20260827` (read-only) into `pharness-data-archive-legacy-20260826`. Manifest `sha256:747cbd3a…5445`, database `sha256:4b10bacc…70d4ee`. An independent read-only Job verified it: database hash matches, `integrity_check=ok`, migrations 1–55 all successful, 14 WorkItems, 82 Runs, and 0 hosted reconciliations, operations, operation locks and agent leases.
- Pin (#421) changes only image digests and revisions, plus the derived agent-execution registry hashes. It was rendered with `values-yfinance-production.yaml`, and all 53 objects passed a server dry-run. The API inference registry JSON is identical to live, and the model-gateway env is unchanged.
- Observed 14:34:09–14:34:55 UTC: Argo `pharness` Synced/Healthy/Succeeded at `c3395e8`. All five Deployments are 1/1 with `observedGeneration` equal to `generation`. Pod imageIDs equal the pinned digests with 0 restarts. Readiness reports API and UI at `97c7e33`, `platform_versions_match=true`, mode `normal`, registry `a2850180…` aligned. Schema is unchanged (latest migration `0055`); there are 14 WorkItems and 82 Runs. `PHARNESS_CODING_RELIABILITY_V2_ENABLED=false`. The hosted workflow env is not rendered, which means `hostedWorkflow.enabled=false`. No rollback was needed.

## Test Diagnosis exact-policy protocol preflight

One POST, following the `lucas-ops` pattern: an exclusive operation record `astra-localprep-protocol-test-diagnosis-20260925` written before the request, and no retry.

- Receipt `inferverify_01a0d8fe43c0787190b720c22848a461`, 14:35:36–14:35:51 UTC (API latency 14.1 s): **failed**, `protocol case single_tool_call attempt 1 did not return exactly one expected native tool call`. The failure is bound to `test-diagnosis-nemotron-v2@v1` (`sha256:ebdd7edb…44f1`), target `fireworks-nemotron-lightning-3p5-30b-a3b@v1` (`sha256:a014784b…8ecf`), registry `a2850180…`, runtime `97c7e33`.
- **The timeout did not recur.** The gateway logged one `upstream request started` (correlation `verify_inferverify_01a0d8fe43c0787190b720c22848a461_1_1`, attempt 1/3) and no warn- or error-level transport event. Its success-path events (headers, stream) are `debug!` under the `info` filter, so a clean round trip shows only the start line. The coding proxy logged nothing in the window. No hop stalled; the provider answered, but the answer did not contain exactly one native tool call.
- Target history across exact-policy checks: timeout on `e508f43`, 30/30 on `d675159`, timeout on `49ecdc5`, and a tool-call-shape failure on `97c7e33`.
- Stop rule applied: no Test Diagnosis qualification, and no Verifier, Builder or Repair protocol check was dispatched. Afterwards readiness was normal and aligned, no evaluation or Job was active, and all Deployments were 1/1.
