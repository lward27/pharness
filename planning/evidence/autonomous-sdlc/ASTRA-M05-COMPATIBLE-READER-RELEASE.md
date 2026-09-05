# ASTRA M05: Compatible reader release and rollback floor

Status: artifacts verified; live rollout pending. Recorded before applying migration 0052 on 2026-09-05.

The earliest retained compatible reader is PHarness source
`2249950d225a4632b24235c2b6f2d8469a774243`. After migration 0052 is applied, recover forward using
this release or a later compatible reader. The previous `fd740927` release knows
only migrations through 0051 and cannot reopen the migrated database. Disabling
hosted creation does not lower that executable-reader requirement. Do not remove
migration records, down-migrate, reset the Finance generation, or restore the
pre-migration snapshot over newer history as routine release recovery.

## Immutable release set

| Component | Digest |
| --- | --- |
| runtime | `sha256:12176080848f9f3e4ffd33873f697e731d2a49077059d900406f909603464171` |
| ui | `sha256:3aa02811edf1b1230f3fb6807a800802d93132ffa34974aa92258c141a6b295e` |
| python-runner | `sha256:058e0e2a9c59fb47a7a89eae567e3f0f87a394e1cf050ab2ebc7d71106672152` |
| node-runner | `sha256:451ed9b3742ea57e2e3f67ad485d0cfd7f1d07a204514204315d3cdd6ff1d8f1` |
| model-gateway | `sha256:9fbc812ec40e2bc071f29f06b8ce7c9561a301308b13a53c2bf77c89aa5fe389` |
| eval-runner | `sha256:2c5a09b6cbe46210e8f63df1c1b061db84300a2b6f154a0f64d752bfd8a79379` |
| codex-host | `sha256:abe8bac8dd1be0927e1b94766b6d1c2aa3e487f8e72cdab346eb5cfecbaf0aa8` |

Registry prefix: `registry.lucas.engineering/pharness-<component>@<digest>`.
All seven images are Linux AMD64, built using the explicit Rancher Desktop builder
from the same clean merged source. Registry content hashes, TLS, operating system,
architecture, source labels and revision labels were verified.
[Build evidence](ASTRA-M05-READER-RELEASE-BUILD.json) links the artifact identities.
This does not claim SBOM, signing or provenance attestations.

The retained native bundle is `pharness-codex-host-2249950d225a4632b24235c2b6f2d8469a774243-linux-amd64.tar.gz`,
SHA-256 `c8e3be60d07d24ecc266ef39a7349a6eb46ba078a1d2357f4bfa7c1e220ebf29`, 231,643,972 bytes.
All 13 file checksums, its source revision, Linux runtime verification target and
absence of AppleDouble metadata passed. Codex is pinned to 0.150.1. The bundle
remains in the source build worktree's `dist/` directory.

## Data preservation before rollout

The running schema-51 database was archived with PHarness's SQLite-aware admin
command, read-only against the source, into
`/data/archives/ASTRA-pre-0052-20260905` on
`pharness-api-data-finance-20260827`. The archive database is 21,204,992 bytes,
SHA-256 `fbc7e2873e71202ae90fcb1fe28e6f160e189e9adc5c29ab395cb39ce7c29d3f`.
It is a same-volume migration snapshot, not an independent disaster recovery backup.
[Archive verification](ASTRA-M05-DATABASE-ARCHIVE-VERIFIED.json) records integrity,
51 applied migrations, generation and record counts without exporting database contents.

The actual immutable reader started successfully in draining mode against an
isolated copy and applied 0052. All 14 WorkItems, 82 Runs, 102 stage executions,
102 stage outcomes, 83 evidence validations, 260 audit events and four retention
holds were preserved. Historical policy fields remain null and generation
`dbgen_finance_20260827` is unchanged. The original database remained at 0051.
[Clone verification](ASTRA-M05-CLONE-MIGRATION-VERIFIED.json) records exact Job,
Pod and container identities. No cluster credentials were mounted in those Jobs.

## Controlled rollout and recovery

Apply through the PHarness Helm source and Argo, using the separate immutable
release-pin commit. Keep `hostedWorkflow.enabled: false` and
`features.codingReliabilityV2.enabled: false`. Confirm active Runs and qualification
Jobs have reached a terminal boundary before merging. Observe the actual Argo
revision, five long-running Deployment image identities, API/UI revisions, schema,
Finance generation, history and retention after reconciliation.

If rollout fails, preserve the database and diagnose the failed boundary. Republish
these compatible image pins through GitOps if a later release is the cause; do not
select an older schema-51 image. The first migration release may require a forward
fix from this compatible source because there is no older compatible deployed binary.
Future migration 0053 requires its own compatible reader floor.

This release prepares the hosted contract. It does not qualify a coding profile,
enable autonomous requests, demonstrate delivery, or authorize Finance production.
Live observation and the remaining milestone gates must be recorded separately.
