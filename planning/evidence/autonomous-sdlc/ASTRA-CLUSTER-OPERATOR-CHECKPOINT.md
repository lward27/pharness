# ASTRA: Cluster operator checkpoint

Recorded 2026-09-06. The owner requested helper/skill cleanup before further M04 implementation. This checkpoint does not close M04 or authorize Finance production.

## Maintained entry point

Use `~/.local/bin/lucas-ops` and the [Lucas Engineering operator guide](https://github.com/lward27/lucas_engineering/blob/main/docs/operations/ASTRA-CLUSTER-DEVELOPMENT.md). Infrastructure [PR 58](https://github.com/lward27/lucas_engineering/pull/58) adds the maintained CLI, profile, versioned skills, CI and acceptance evidence. Installed content hash: `59716f40df8ba6169cacee2819c0c37afc8487926363f9305bc594f33da2e7d0`.

Twenty-five deterministic failure/recovery tests and the CI check pass. Installed live readiness, owned-tunnel recovery, an actual uncached AMD64 preflight and verification of the existing seven-image/native-bundle release pass. These checks do not prove coding reliability, autonomous delivery or Finance acceptance.

All 91 inactive one-off temporary ASTRA Python helpers are retained privately at `/Users/wardl/.local/state/lucas-ops/legacy/20260906T203053Z`. Their prior temporary entry points were removed after byte-hash and process checks. Use the maintained interface for repeated operations; consult the [archive inventory](https://github.com/lward27/lucas_engineering/blob/main/docs/operations/ASTRA-LEGACY-HELPER-ARCHIVE.json) for historical forensics. No evidence or credential files were erased.

## Exact M04 resume point

Compiled source remains `92f8f1b8e98dd45d0a01e030aeb99ef9bcf95267`; observed release pin was `4bf9f0ae73a3ed2ef25e9bca53090e0edecf32f6`. Schema 55 remains compatible only with source 92 or later compatible readers. The native inference registry hash is `sha256:e05f943fcb3a870c4a3a145ab3b06849a36e8a3c13d262cdd28e49394719bd79`.

The first deployed diagnostic, `infeval_01a07730b1e275e096ba85c8afb04d4c`, is terminal. Its native operation status is `completed`, but the diagnostic **failed**; qualification is null and infrastructure validity is false. It used one primary MiniMax Onboarding `python-contract` case. Protocol calibration passed 30/30. Its detailed result reports `stage_scope_or_coverage: undeclared_onboarding_write_scope`. Preserve the distinction between a completed evaluation and a passing evaluation.

The [recovered native result](ASTRA-M04-92F8F1B-ONBOARDING-PRIMARY-RECOVERED.json) was retrieved after Kubernetes expired Job `pharness-inference-eval-59996a313b7d`; no replacement model run was started. Inspect the retained submission, declared case scope and native grading path before choosing a correction or an exact-input control. Do not infer model incapability from this one classification or start another broad batch. The frozen benchmark thresholds and budgets remain unchanged.

```sh
~/.local/bin/lucas-ops evaluation status infeval_01a07730b1e275e096ba85c8afb04d4c --output-dir /absolute/evidence/path
```

M04E connected coding/repair and M04F full qualification remain open. The prepared Finance production-baseline change is still unapproved. M08/M09 drafts remain unaccepted; no M11 request or approval was performed.

## Concurrent infrastructure cleanup

The owner separately authorized retirement of Odoo, Clawspace/OpenClaw, Epheros, Uptime Kuma and Code Server through `lucas_engineering`. The [retirement record](https://github.com/lward27/lucas_engineering/blob/main/docs/operations/ASTRA-APPLICATION-RETIREMENT.md) owns that scope and evidence. PHarness, Finance, Tekton, LGTM, registry and shared databases remain outside that retirement. LEA cluster retirement remains a read-only assessment requiring a separate decision.

The user is removing old failed Pods manually. The three failed ASTRA Job Pods have terminal failed owners and their [bounded logs/status are preserved](ASTRA-FAILED-DIAGNOSTIC-POD-PRESERVATION.json); Pod deletion is safe while retaining Jobs and PVCs. No Pod deletion was performed by this task.
