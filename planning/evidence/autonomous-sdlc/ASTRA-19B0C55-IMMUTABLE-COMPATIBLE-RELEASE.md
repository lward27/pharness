# ASTRA: Compatible release for corrected stage qualification

Status: complete artifacts, isolated migration validation, exact live rollout and live database preservation passed. Fresh qualification remains required. This record does not accept autonomous delivery.

The [live observation and preservation record](ASTRA-19B0C55-LIVE-PRESERVATION.md) documents the accepted deployment, retained initial checker failure and narrowly verified operational timestamp differences.

## Exact release boundary

All seven images and the native bundle were built from merged PHarness source `19b0c55c48e3614d0b4507d56df3029a52475618`, after the complete workspace tests passed. [Artifact verification](ASTRA-19B0C55-RELEASE-BUILD.json) checks returned immutable digests, Linux AMD64 configuration, matching source/revision labels and independently hashed registry manifests/configuration over verified TLS. No SBOM, signature or cryptographic provenance attestation is claimed. Every layer was not independently downloaded and hashed by the release inspector.

| Component | Immutable digest |
| --- | --- |
| runtime | `sha256:1e63d8f1c7fa34bcd390fa80747fc8705b0631e750029d62bb2a8890dffd7709` |
| ui | `sha256:1e8f81082340ec5ec98351e6eb5df0496b9b1c1b472c22ef6165237355f4999f` |
| python-runner | `sha256:0b06a7154752d736cee504ddc76722681be0d45fd366ddc8e6c25db275bbbf54` |
| node-runner | `sha256:e2be42778757260a611824e15ebe1be6dc6537c2fd073bcadd94a4c7bd284338` |
| model-gateway | `sha256:877c35c69c2cfcc0d3a9c16a351111b298294ad55e7cf30c875e3f2ad204f1b7` |
| eval-runner | `sha256:8d73540d5a3cd7d05a849ae2ca444146ba00fa91bbf9fa6e06653f818de4baa6` |
| codex-host | `sha256:0d5d8af760236075effa76240f606f2acbb73eb3ca616214a52ff8f0c91bfb0d` |

The [native bundle verification](ASTRA-19B0C55-NATIVE-BUNDLE-VERIFIED.json) checks all 13 files, their checksums, the source revision, Linux AMD64 ELF headers and the existing container-based bundle-verification target. The archive is retained at `/Users/wardl/Personal/apps/pharness-release-artifacts/19b0c55c48e3614d0b4507d56df3029a52475618/pharness-codex-host-19b0c55c48e3614d0b4507d56df3029a52475618-linux-amd64.tar.gz`, SHA-256 `10e93e906da13ab1e433558e7f260feaefa1d4bca1a4417d19c43ba8ffe5238e`. The native Kubernetes host deployment stays disabled; the existing host control-plane flag remains unchanged.

## What ships and what remains gated

This source contains the corrected Onboarding, Planner and Test Diagnosis contracts/qualification fixtures; bounded accepted stage submissions and separate Verifier diagnostics; guarded exact-source merge and durable build/staging handoffs; schema-54 delivery records; and the reviewed console corrections. Newer Finance identity, signal-window and health-trace readers are separate draft work and are not in these artifacts.

Hosted workflow creation and Coding Reliability V2 remain disabled. Preserve the single writer, Finance data generation, retention/audit history and execution limits. Qualification evidence from `48c77b7` remains historical: Builder and seeded Repair passed, while Onboarding, Planner, Test Diagnosis and Verifier failed. This release must collect its own evidence; source fixes or artifact readiness cannot relabel those failures as success.

## Compatibility and observed failures

The [pre-54 archive](ASTRA-19B0C55-PRE0054-ARCHIVE-VERIFIED.json) records schema 53 and the existing Finance generation on its current PVC. It is a same-volume archive, not independent disaster recovery. An isolated scratch copy ran the actual candidate API migrator. [The accepted clone check](ASTRA-19B0C55-CLONE-MIGRATION-VERIFIED.json) confirms schema 54, database integrity, no foreign-key violations and identical original-column fingerprints across all 81 prior tables, with the original archive unchanged.

The first clone verifier failed after migration because its query looked for `delivery_stage` on the GitOps table rather than joining the deployment table. [That verifier failure](ASTRA-19B0C55-CLONE-MIGRATION-R1-FAILURE.json) is retained. The corrected check preserved all history/integrity requirements and passed. No live database was migrated by either clone test.

The initial Mac release build failed under its observed 4 GiB VM memory limit before publishing any image. The saved 6 GiB setting was not observed as active capacity. The [desktop restoration](ASTRA-M02-DESKTOP-BUILDKIT-RETURN.md) restored the existing native AMD64 worker and cluster endpoint through GitOps. A desktop client configuration error occurred before dispatch; later, a public Python registry read was refused by Cloudflare after the runtime image had already been pushed. The exact pushed digest was verified through the trusted private TLS route, without rebuilding it or disabling certificate verification. All failures retain separate evidence.

## Rollout and recovery gate

The reviewed release changes only Helm image/revision pins, the required agent registry image references and their derived hashes, and documentation/evidence. The [release validation](ASTRA-19B0C55-RELEASE-PIN-VALIDATION.json) verifies exact substitutions, unchanged limits and disabled gates, and strict server-side validation of all 50 rendered resources. Argo must observe the exact merged pin revision; API/UI/gateway and both egress proxies must run the expected digests, with ready Service endpoints. Configured worker and runner identities must agree. These live identity checks and the separate read-only schema/history check passed before qualification started; see the linked live preservation record.

**Schema 54 establishes a new rollback floor as soon as migration runs, even with zero hosted WorkItems.** Source `48c77b7` is not a compatible rollback reader. This source (`19b0c55`) is the first observed release validated against schema 54 and is now the minimum compatible rollback reader. Recovery requires a compatible reader/forward repair or an explicitly reviewed archive restoration. Do not roll back code to a reader that refuses or misinterprets the current schema.

No Finance production image, production approval, source-branch protection, hosted WorkItem or destructive recovery is part of this PHarness release. M04 qualification and all remaining end-to-end gates stay open.
