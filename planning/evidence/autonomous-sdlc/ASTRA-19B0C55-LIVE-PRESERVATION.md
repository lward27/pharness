# ASTRA: Live schema-54 preservation and release observation

Status: compatible PHarness release observed; live Finance/history preservation passed. M04 qualification and autonomous SDLC acceptance remain open.

[Release PR 359](https://github.com/lward27/pharness/pull/359) merged at 23:45:35 UTC on 2026-09-05. Argo auto-sync applied exact pin `f9909da0fc45ef47ed03bbffbfe5dc8fc31c62da`. At 23:48:08 UTC, the [release observer](ASTRA-19B0C55-RELEASE-OBSERVED.json) verified source `19b0c55c48e3614d0b4507d56df3029a52475618`, all five current Deployment/Pod image identities, ready Service endpoints, zero restarts, the configured worker/runner images and the retained Finance generation. Historical failed Pods remain retained and were not counted as current release failures. The old local API connection ended during replacement; a new connection was established before API checks passed.

The [read-only live preservation check](ASTRA-19B0C55-LIVE-DATABASE-VERIFIED.json) passed in Job `astra-live-schema0054-preserve-20260905`, completed at 23:53:17 UTC. It verifies schema 54, integrity, zero foreign-key violations, zero hosted rows, the unchanged archive hash and every original row across all 81 prior tables. Application, delivery, qualification and audit history is byte-for-byte identical in its original columns. No checker mounted credentials or a writable data volume; no checker applied migrations or repaired data.

## The initial failure is retained

The [first check failed](ASTRA-19B0C55-LIVE-DATABASE-R1-FAILURE.json) because it required whole-table equality against an archive made at 21:08 UTC, while the existing native-host control plane continued to receive heartbeats. The [complete difference inventory](ASTRA-19B0C55-LIVE-DATABASE-DIFFERENCES.json) found exactly three affected tables:

| Table | Observed difference | Accepted boundary |
| --- | --- | --- |
| `agent_host_capability_snapshots` | 219 to 231 rows | All 219 original snapshots remain identical. New snapshots belong to existing hosts and have valid later timestamps. |
| `agent_hosts` | One row: `last_contact_at`, `updated_at` | Only these two timestamps may advance. Identity, credential hash, lifecycle and all other fields remain identical. |
| `organizations` | One row: `updated_at` | Only the startup/bootstrap timestamp may advance. Organization identity, display name and all other fields remain identical. |

These are explicit operational differences, not a claim that every live column stayed unchanged. The native host [heartbeat handler](../../../crates/pharness-api/src/app/agent_hosts.rs) calls the existing [heartbeat/snapshot store methods](../../../crates/pharness-store/src/sqlite/agent_execution.rs); API startup calls the existing [organization bootstrap](../../../crates/pharness-store/src/sqlite/product.rs). The accepted check rejects deleted original rows, changes to original capability snapshots, changes outside the three timestamp fields, nonmonotonic/future timestamps, new host identities, or any other table difference. It separately fingerprints all stable columns in the two mutable records. The isolated migration check still passed exact equality across all 81 original tables; these live differences are normal operation between the archive and the observation.

## Compatibility and remaining gates

Source `19b0c55` is now the minimum observed compatible rollback reader for schema 54. An older schema-53 binary is unsafe even with zero hosted work; use compatible forward repair or separately reviewed archive restoration if recovery is needed. The archive is retained on the existing PVC and is not independent disaster recovery.

Hosted creation, Coding Reliability V2 and native Kubernetes hosts remain disabled. The existing native host control plane remains enabled, with its original behavior and limits. Fresh Onboarding qualification was initiated only after this preservation check passed. Historical qualification failures remain failures. Finance production was not changed, and the newer native Finance runtime readers remain draft implementation pending integration and their own deployment.

The [Onboarding protocol calibration](ASTRA-M04-19B0C55-ONBOARDING-LIVE-PROTOCOL.json) passed all 30 required cases. [Two independent semantic attempts](ASTRA-M04-19B0C55-ONBOARDING-LIVE-START.json) are running as `infeval_01a073fe89947b33a1e8d1a24b7dc200`, Job `pharness-inference-eval-2a7ec5055565`. This is dispatch evidence, not a qualification pass.
