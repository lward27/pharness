# ASTRA M07: Verified source-to-build handoff

Status: implementation and offline validation complete for this bounded handoff slice; durable automatic build dispatch and milestone acceptance remain open. Source base: `2bbc7a77152d4104651702e84bac3b1893739fc3` on 2026-09-05. This document is committed with its implementation.

## Implemented behavior

Hosted pipeline context consumes the sealed source-delivery outcome instead of requiring the older coding-Run Git delivery artifacts. It checks the current approved plan and ChangeSet, five bound engineering outcomes, the saved workflow hash, recorded source operation, autonomous merge admission, exact base/head/merge ancestry, and provider-check observation. An unadmitted merge, missing source outcome or changed evidence cannot fall back to the source-only contract. The source intent and sealed outcome must agree.

A ChangeSet binds the engineering evidence that preceded source delivery. Recording later source, release or observation outcomes no longer incorrectly makes that ChangeSet stale. Changed engineering evidence still invalidates it.

Build preflight revalidates the complete saved PipelineContract, finite Finance source/pipeline/image coordinates, exact merge revision, dedicated 1Gi workspace and non-deploying execution. Its authorization is the saved workflow's automatic Build permission; the overall WorkItem's future production impact does not grant production authority to a build. Pause or contract retirement blocks execution. Existing manual execute and envelope routes explain that hosted progression belongs to its controller. Source-only work retains its original grants and gate behavior.

The Tekton reader now reads declared PipelineRun results as well as historical TaskRun results. It exposes `SOURCE_COMMIT`, the original declared results, Kubernetes UIDs and conflicts across reported source/image values. Hosted build output requires all three declared results, the exact merged source, the finite application's `git-<commit>` image tag, a valid digest, successful terminal status and no conflicting results. Missing declared outputs and inventory-only data cannot produce a verified hosted build artifact. Historical TaskRun-result fallback remains readable.

## Validation and limitations

The complete API/core and worker checks passed **464 tests**: 270 API, one administration check, 148 core unit, six core-type, two hosted-contract and 37 worker tests. New regression cases cover source without legacy Run artifacts; unsealed/changed source; finite non-deploying authority; rejected manual authority extension; wrong repository pipeline, namespace, revision and contract version; pause/resume and contract retirement; missing, mismatched and conflicting declared results; and retained secret redaction. Clippy with warnings denied, formatting, diff checks and the architecture checks passed. Exact source-file and log hashes are in the [validation record](ASTRA-M07-HOSTED-BUILD-HANDOFF-VALIDATION.json).

The first integration run exposed the old ChangeSet/downstream-outcome assumption. An intermediate check also compared an artifact hash with the hash of only its JSON body; the existing store hashes the full artifact envelope. The corrected check uses the sealed artifact reference/hash and validates its bound authority. These failures occurred in local fixtures and were corrected before release.

**No automatic build was dispatched by this slice.** The controller still needs a persisted build operation, bounded admission immediately before external creation, original-identity Job/PipelineRun recovery and duplicate-safe outcomes. Registry/source correlation and actual autonomous delivery must be demonstrated through that implementation. The earlier [real Finance builds](ASTRA-M07-SOURCE-DELIVERY-AND-BUILDS.md) remain program-operated evidence, not M11 acceptance. Unit fixtures are not production or live-build proof.

## Deployment and recovery

There is no database migration or new endpoint in this slice. Keep hosted creation disabled while the remaining controller, qualification, branch-protection and release gates are open. Release the required complete image set and native bundle from one merged source revision after active evaluations finish. Record the compatible rollback floor before enabling hosted build writes; an older reader must not reinterpret hosted work as source-only completion. Preserve operation and evidence records during recovery.

The [program](../../programs/autonomous-sdlc/ASTRA-00-PROGRAM.md) owns current milestone status. Production approval remains a separate human event before the production GitOps merge.
