# ASTRA M06: Controller release and schema 53 recovery floor

Status: artifact and clone checks passed. The recovery floor was committed before the live migration. Release PR 339 merged at 16:14:59 UTC as `0bc84048e0d8817c6451e6f83dfcf250a17ab3b5`; live images and schema/history preservation are verified.

The earliest retained release that understands migration 0053 is source `48c77b7b4438d621ff9563b913857bcf771f1800`. After 0053 is applied, recovery must use this release or a later compatible reader. The previous `2249950d225a4632b24235c2b6f2d8469a774243` release cannot reopen this schema. Disabling hosted creation does not make the older reader compatible. Do not remove migration records, down-migrate, reset the Finance generation or restore the snapshot over newer history as routine recovery.

## Immutable artifact set

| Component | Digest |
| --- | --- |
| runtime | `sha256:42d7acbc2e425c76c7b22be58251aa2bb45f5a94b25634421f30f9d7c4dabf6d` |
| ui | `sha256:1324f357883c4d9930dc6c9cbecd27c5ab316646774eefd24973f96f2768e3ef` |
| python-runner | `sha256:e7992e58091d7f5857c3034afddaefd1466e978a32b6f4b78280aa6937eb514b` |
| node-runner | `sha256:81074b9f17683377026c124ded3c3af3beed8715058e13827a6d1215e8894c5f` |
| model-gateway | `sha256:b8ba037ab2f8c59bc6f81661676a4bbeefb27b655f8b2d1799eba9025d8b2b63` |
| eval-runner | `sha256:15f0f1a1dd392b209283a4fe84337e8d6899ed5bba1dbaa16ba768a580ba0910` |
| codex-host | `sha256:994ff6440a5ebbb63c8e7c457bc6956b1e911dac1499ed4809be1484ee212130` |

Registry prefix: `registry.lucas.engineering/pharness-<component>@<digest>`. All seven images were built from the same clean merged source using the explicit Rancher Desktop builder. The uncached AMD64 execution preflight, content hashes, trusted TLS, registry resolution, Linux architecture and source labels passed. [Build record](ASTRA-M06-CONTROLLER-RELEASE-BUILD.json) binds their identities. Content identity is not an SBOM, signature or cryptographically verified provenance claim.

The native bundle `pharness-codex-host-48c77b7b4438d621ff9563b913857bcf771f1800-linux-amd64.tar.gz` is retained at `/Users/wardl/Personal/apps/pharness-release-artifacts/48c77b7b4438d621ff9563b913857bcf771f1800/pharness-codex-host-48c77b7b4438d621ff9563b913857bcf771f1800-linux-amd64.tar.gz`. Its SHA-256 is `a0f88d77dd55d8cf1234c6709fff9792831c95e2d59a59799a336708e4cb7013`; it is 231,643,979 bytes. All 13 checksums and the Linux runtime verification target passed, with Codex 0.150.1 and no AppleDouble entries. [Native evidence](ASTRA-M06-NATIVE-BUNDLE-VERIFIED.json).

## Data preservation and rollout

The schema-52 snapshot is `/data/archives/ASTRA-pre-0053-20260905` on the existing Finance PVC. Its database is 21,213,184 bytes, SHA-256 `abdb41c531ff054f56537907925cc8d974d79f40f7cc3c509a4c0ef0878df6d4`. It is a same-volume migration snapshot, not independent disaster recovery. [Archive verification](ASTRA-M06-DATABASE-ARCHIVE-VERIFIED.json).

The exact new runtime successfully applied 0053 to an isolated copy. All 14 WorkItems, 82 Runs, 102 stage executions, 102 stage outcomes, 83 evidence validations, 260 audit events, four retention holds, one product, five repositories and the Finance generation were preserved. Historical WorkItem fields were compared exactly. Hosted controller tables stayed empty and historical work was not enrolled. The original snapshot remained at 0052. [Clone verification](ASTRA-M06-CLONE-MIGRATION-VERIFIED.json).

Deploy through a separate Helm release-pin commit and observe Argo's actual reconciliation. Keep hosted creation and Coding Reliability V2 disabled. Check that no active Runs or qualification Jobs will be interrupted, then verify the exact five long-running Deployment image identities and API/UI revisions. Before any new qualification or contract write, compare the live database read-only against the retained snapshot, including schema, history, generation and retention holds.

This release includes the engineering controller, prompt clarification and initial console changes merged through source 48c77b7. It excludes later build-dispatch, source-progression and list-polish changes. It does not qualify the coding path or prove autonomous delivery. Finance production approval remains a separate human action before its GitOps merge.

If this first schema-53 deployment fails, preserve the data and use a forward fix from this compatible source. For a later compatible release regression, restore these exact pins through GitOps only after checking configuration and data compatibility. Never deploy a schema-52 reader against the migrated database.

## Release dispatch

[PR 339](https://github.com/lward27/pharness/pull/339) contains the exact release pins and the recovery floor. The preflight found all 82 Runs terminal and no active Jobs. All 53 rendered Kubernetes objects passed server dry-run. A hard repository-cache refresh was requested; Argo remains responsible for automatic reconciliation. No manual sync receipt is used as deployment evidence.

## Live verification

At 16:19 UTC, Argo reported `Synced` and `Healthy` at release revision `0bc84048e0d8817c6451e6f83dfcf250a17ab3b5`. All five long-running Deployments ran the expected image identities with ready Pods and zero restarts. The [observation record](ASTRA-M06-CONTROLLER-RELEASE-OBSERVED.json) distinguishes the 62 retained historical failed API Pods from the current release.

At 16:20 UTC, the [read-only live database comparison](ASTRA-M06-LIVE-DATABASE-VERIFIED.json) confirmed all 53 migrations, identical historical WorkItem fields and every recorded history/retention count unchanged. All three hosted controller tables remain empty. No new qualification or contract write preceded this check. Hosted creation and Coding Reliability V2 remain disabled. The first fresh gateway qualification request followed the successful comparison.
