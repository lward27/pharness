# ASTRA M04: Live diagnostic release and preserved Finance history

Recorded: 2026-09-06T14:51:55.326981+00:00. Compiled source: `92f8f1b8e98dd45d0a01e030aeb99ef9bcf95267`.
Release PR: [372](https://github.com/lward27/pharness/pull/372). Observed Argo revision: `4bf9f0ae73a3ed2ef25e9bca53090e0edecf32f6`.
Status: compatible diagnostic release deployed and internal service checks passed; M04 qualification and autonomous Finance acceptance remain open.

## Objective evidence

[Live release identity](ASTRA-M04-DIAGNOSTIC-RELEASE-OBSERVED.json) verifies all five deployment generations and Pod image IDs, three serving endpoint identities, configured worker/evaluator and Python/Node runner images, and matching API/UI revisions. API and gateway agree on inference registry `sha256:e05f943fcb3a870c4a3a145ab3b06849a36e8a3c13d262cdd28e49394719bd79`. The seven images and native bundle remain linked by the [immutable build record](ASTRA-M04-DIAGNOSTIC-RELEASE-BUILD.json).

[The read-only live database check](ASTRA-M04-DIAGNOSTIC-LIVE-DATABASE-VERIFIED.json) passes integrity and foreign-key validation at schema 55. All original application and audit data is preserved across 81 tables. The only differences are source-validated operational timestamps and later capability snapshots for already enrolled hosts; every original snapshot and stable host/organization field is retained. All 49 historical evaluations have explicit full-qualification scope. The original schema-54 archive is unchanged. No hosted or diagnostic WorkItem/evaluation writes existed at this preservation checkpoint.

[The complete service window](ASTRA-M04-DIAGNOSTIC-RELEASE-SERVICE-WINDOW-R2.json) covers 300.79 seconds with 11 console/API checks. Exact API/UI revisions and gateway alignment stayed consistent. All five deployments had fresh available-replica and zero-restart signals. CPU, memory and throttling were recorded without inventing an SLO. Four Pod log streams were present, including the required API and UI streams; the declared severe-log search found no matches. Pod identities remained unchanged and no Pod warning events were observed.

## Failures retained and limits

The [first observation attempt](ASTRA-M04-DIAGNOSTIC-RELEASE-SERVICE-WINDOW.json) ended before its first HTTP sample because both local forwarding connections had closed. [The recovery record](ASTRA-M04-DIAGNOSTIC-OBSERVATION-TRANSPORT-RECOVERY.json) confirms that the five deployed Pods remained ready with identical UIDs and zero restarts. The underlying transport cause is not established. Fresh tunnels and a new full window were required; no application restart or rollback was performed.

The [task-local diagnostic launcher](ASTRA-M04-DIAGNOSTIC-LAUNCHER-HASH-PREFLIGHT.json) initially compared deliberately blank source-registry hash slots with native runtime hashes. It stopped before any model POST. The corrected launcher checks the exact registry retained in the verified release and its computed target identities. No deployed code or qualification gate changed.

An unauthenticated public console probe returned HTTP 403 before the image pin, and the public API hostname did not resolve from this computer. This report proves internal service behavior over authorized cluster connections, not public ingress acceptance. It does not assert application traces or a latency SLO for PHarness. The 62 terminal API Pods at this observation predate the release. The owner later began Pod cleanup; their old count is historical, not a current rollout-failure count.

## Compatibility and next work

`92f8f1b` is now the minimum compatible database reader. Schema-54 binaries cannot open the live schema-55 store safely. Keep the verified archive and compatible artifacts; do not undo this migration by rolling images back to source 19.

M04A–D are deployed. The next gate is the small real-gateway stage diagnostics and their explicit same-input controls. They cannot create policy qualifications. The connected coding/repair proof and full frozen qualification remain separate requirements. Hosted creation, Coding Reliability V2 and Kubernetes native-host execution remain disabled. This release creates no Finance production approval or M11 success claim.

The first live source-92 diagnostic later completed with a failed result and no qualification. The [operator checkpoint](ASTRA-CLUSTER-OPERATOR-CHECKPOINT.md) records its exact resume point and the maintained replacement for temporary launchers.
