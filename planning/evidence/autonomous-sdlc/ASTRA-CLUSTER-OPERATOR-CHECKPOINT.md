# ASTRA: Cluster operator checkpoint

Recorded 2026-09-06. The owner requested helper/skill cleanup before further M04 implementation. This checkpoint does not close M04 or authorize Finance production.

2026-09-07 continuation: [local-model readiness and Onboarding scope analysis](ASTRA-M04-LOCAL-MODEL-READINESS.md) records inspection of the retained failure, the prepared shared prompt/transport corrections, validation and the next release/canary gate. The source-92 observations below remain historical evidence; no new qualification is implied.

Latest continuation (2026-09-08): [source38 is released](ASTRA-M04-DIAGNOSTIC-SOURCE-RELEASE.md), with exact serving/worker images, preserved data and a passing ten-minute window. The [primary/control onboarding comparison](ASTRA-M04-38A4282-ONBOARDING-CONTROL.md) proves identical inputs and the repaired native source reader. Each model passes one of two cases. Share deterministic Product Service validation with the submission tool before the next runtime canaries. [The Mac Tekton route is restored](ASTRA-M02-MAC-BUILDKIT-RETURN.md) while lucas-desktop is off. No local model is activated.

## Maintained entry point

Use `~/.local/bin/lucas-ops` and the [Lucas Engineering operator guide](https://github.com/lward27/lucas_engineering/blob/main/docs/operations/ASTRA-CLUSTER-DEVELOPMENT.md). Infrastructure [PR 58](https://github.com/lward27/lucas_engineering/pull/58) adds the maintained CLI, profile, versioned skills, CI and acceptance evidence. [PR 62](https://github.com/lward27/lucas_engineering/pull/62) updates the selected Mac builder and truthful inspection semantics. Installed content hash: `169f93c6f2e8ed26f2f9b5088ab5647bbc3741a7c442dea8cb123cda0c8da4c0`; the prior version remains retained.

Twenty-eight deterministic failure/recovery tests and the CI check pass. Installed live readiness, owned-tunnel recovery, an actual uncached AMD64 preflight and verification of the existing seven-image/native-bundle release pass. These checks do not prove coding reliability, autonomous delivery or Finance acceptance.

All 91 inactive one-off temporary ASTRA Python helpers are retained privately at `/Users/wardl/.local/state/lucas-ops/legacy/20260906T203053Z`. Their prior temporary entry points were removed after byte-hash and process checks. Use the maintained interface for repeated operations; consult the [archive inventory](https://github.com/lward27/lucas_engineering/blob/main/docs/operations/ASTRA-LEGACY-HELPER-ARCHIVE.json) for historical forensics. No evidence or credential files were erased.

## Exact M04 resume point

Compiled source is `38a4282c0bef27491a8e6e44640dbbe3f33526ca`; observed release pin is `81de84f70393c9dd9dad302930a97acf88da4ec9`. Schema 55 and the source-92-compatible reader floor remain unchanged. The inference registry hash remains `sha256:e05f943fcb3a870c4a3a145ab3b06849a36e8a3c13d262cdd28e49394719bd79`.

Primary `infeval_01a08138694073d1b8831ae909f83122` and exact-input control `infeval_01a081417d0173f3a8a7f93ad191c09b` are both terminal. Each passes 1/2, with no qualification. Final Pod/image/exit receipts are saved. MiniMax fails the blocked candidate's environment identifier; Kimi correctly blocks that case but proposes creation of an existing Service in the valid case. The [comparison and next implementation boundary](ASTRA-M04-38A4282-ONBOARDING-CONTROL.md) own the evidence. Historical source-a2 success and source-81/source-92 failures remain unchanged.

Next: reuse deterministic Product Service/reference checks at the native submission boundary with precise creation-versus-binding semantics and original Run context. Preserve fresh API revalidation, valid new-Service proposals, blocked null candidates, budgets and immutable history. Use the actual retained control for a regression. Release the correction before fresh exact-runtime diagnostics; remaining stage canaries have not been dispatched. Do not select a model from these two mixed cases.

Historical first deployed diagnostic, `infeval_01a07730b1e275e096ba85c8afb04d4c`, is terminal. Its native operation status is `completed`, but the diagnostic **failed**; qualification is null and infrastructure validity is false. It used one primary MiniMax Onboarding `python-contract` case. Protocol calibration passed 30/30. Its detailed result reports `stage_scope_or_coverage: undeclared_onboarding_write_scope`. Preserve the distinction between a completed evaluation and a passing evaluation.

The [recovered native result](ASTRA-M04-92F8F1B-ONBOARDING-PRIMARY-RECOVERED.json) was retrieved after Kubernetes expired Job `pharness-inference-eval-59996a313b7d`; no replacement model run was started. Inspect the retained submission, declared case scope and native grading path before choosing a correction or an exact-input control. Do not infer model incapability from this one classification or start another broad batch. The frozen benchmark thresholds and budgets remain unchanged.

```sh
~/.local/bin/lucas-ops evaluation status infeval_01a07730b1e275e096ba85c8afb04d4c --output-dir /absolute/evidence/path
```

M04E connected coding/repair and M04F full qualification remain open. The prepared Finance production-baseline change is still unapproved. M08/M09 drafts remain unaccepted; no M11 request or approval was performed.

## Concurrent infrastructure cleanup

The owner separately authorized retirement of Odoo, Clawspace/OpenClaw, Epheros, Uptime Kuma and Code Server through `lucas_engineering`. The [retirement record](https://github.com/lward27/lucas_engineering/blob/main/docs/operations/ASTRA-APPLICATION-RETIREMENT.md) owns that scope and evidence. PHarness, Finance, Tekton, LGTM, registry and shared databases remain outside that retirement. LEA cluster retirement remains a read-only assessment requiring a separate decision.

The user is removing old failed Pods manually. The three failed ASTRA Job Pods have terminal failed owners and their [bounded logs/status are preserved](ASTRA-FAILED-DIAGNOSTIC-POD-PRESERVATION.json); Pod deletion is safe while retaining Jobs and PVCs. No Pod deletion was performed by this task.
