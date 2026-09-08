# ASTRA M04: Release corrected measurements and exact-policy verification

Status: source, immutable artifacts, release pins, exact live identities and the full ten-minute service window pass. Model diagnostics, qualification and Finance acceptance remain separate gates.

Source `d675159cb205d222616d253d311706be1889bb91` combines the [measurement/completeness correction](ASTRA-M04-MEASUREMENT-CONTRACT-CORRECTIONS.md), [full semantic-fixture audit](ASTRA-M04-SEMANTIC-FIXTURE-VALIDITY.md) and [policy-specific protocol gate](ASTRA-M04-POLICY-BOUND-PROTOCOL.md). Planner uses v2.5 and Verifier v2.4. The frozen coding and repair tasks, model defaults and execution limits are unchanged. [Release PR393](https://github.com/lward27/pharness/pull/393) carries pin commit `4a6e511cf35b8ad468cbcd18370d881ed4c760bb`, merged as `ce98a26065cd48323b58b8905fcbbfc6c76452cd`.

## Artifact and release evidence

The [original single build](ASTRA-POLICY-PROTOCOL-BUILD-OPERATION.json) completed successfully in 2,176 seconds on the selected Rancher Desktop Mac builder, after an [uncached AMD64 execution preflight](ASTRA-POLICY-PROTOCOL-AMD64-PREFLIGHT.json). [All seven registry images](ASTRA-POLICY-PROTOCOL-ARTIFACTS-VERIFIED.json) have matching source/revision labels and verified manifest/config digests. The [native bundle](ASTRA-POLICY-PROTOCOL-RELEASE-BUILD.json) has outer SHA256 `2335d492b7a26b236d50a0bb6934690b138f0a99934b271e31919ae142043772`, 13 valid internal checksums, the matching revision and no Mac resource metadata. SBOMs, signatures and provenance attestations are not claimed.

Six rendered client TLS warnings and a confirmed internal gateway-to-registry connection timeout are retained in the [transport investigation](ASTRA-M04-D675159-REGISTRY-TRANSPORT.md). The same original build completed; no replacement build or infrastructure change was used. Subsequent valid TLS/HTTP probes and all 20 internal upstream probes passed, but the intermittent cause remains unresolved. The [intermediate source5a40945 artifacts](ASTRA-M04-5A40945-ARTIFACT-BUILD.md) remain held and were not deployed or mixed into this release.

The [pin preparation](ASTRA-POLICY-PROTOCOL-RELEASE-PIN.json) and [reviewed registry diff](ASTRA-POLICY-PROTOCOL-REGISTRY-DIFF.json) change only image identities and derived execution hashes. [Strict Helm lint and server dry-run](ASTRA-POLICY-PROTOCOL-RELEASE-PREFLIGHT.json) pass for the actual Argo overlay's 53 resources in their correct scopes. No model, default, limit, provider, hosted-activation or Finance production change is included.

## Live observation and compatibility

The [five-minute baseline](ASTRA-POLICY-PROTOCOL-BASELINE-SERVICE-WINDOW-R2.json) passes with 11 samples. [Compatibility](ASTRA-POLICY-PROTOCOL-COMPATIBILITY.json) and [activation evidence](ASTRA-POLICY-PROTOCOL-ACTIVATION-BASELINE.json) preserve Finance generation `dbgen_finance_20260827`, 82 Runs, schema55, disabled hosted creation and disabled Coding Reliability V2. There is no SQL migration in this source change.

[Argo auto-sync and exact live identities](ASTRA-POLICY-PROTOCOL-RELEASE-OBSERVED.json) verify all five serving Deployments, three ready Service endpoints, configured worker/evaluator images, both active language runners, matching API/UI revision and unchanged Finance history. No redundant manual sync or rollout restart was used. [Three readiness samples](ASTRA-POLICY-PROTOCOL-WINDOW-START-READINESS.json) waited for all deployment metrics to postdate readiness before starting the full service window. Waiting for fresh data does not count toward its 600 seconds or relax any threshold.

The earliest compatible reader remains source `92f8f1b` or a later compatible reader. The immediate preceding verified runtime is e508. Older protocol implementations can read the additive JSON but lack the new exact-policy admission gate: if rollback is necessary, keep qualification and activation suspended until that gate is restored. This PHarness platform release is owner-authorized; it is not the human Finance production-approval event required by M11.

The [full 600-second service window](ASTRA-POLICY-PROTOCOL-RELEASED-SERVICE-WINDOW-R2.json) passes with 21 direct samples, fresh deployment/restart metrics, API/UI log delivery, no severe-log count or Pod warning, and unchanged Pod identities. The [read-only live policy projection](ASTRA-POLICY-PROTOCOL-LIVE-PROJECTION.json) confirms all 29 policies require current-runtime checks while five historical Kimi target verifications remain readable. Those older passes do not satisfy the new gate. This accepts the internal PHarness release observation, not model qualification or Finance runtime behavior.

## Diagnostic results and next validation boundary

The [declared sequence](ASTRA-POLICY-PROTOCOL-CANARY-SEQUENCE.json) is complete; the [read-only closeout](ASTRA-M04-D675159-CANARY-CLOSEOUT.json) confirms all six evaluations terminal and no active latest evaluation. Each primary/control received its own current-runtime exact-policy30/30 check, and each control used complete valid retained primary inputs.

- [Planner](ASTRA-M04-D675159-PLANNER-COMPARISON.md): native2/2 for both same-model runs, but a consequential unresolved-decision gap prevents treating the plans as safe automatic authority.
- [Verifier](ASTRA-M04-D675159-VERIFIER-COMPARISON.md): GLM2/3, including one malformed terminal submission, versus Kimi3/3.
- [Diagnosis](ASTRA-M04-D675159-DIAGNOSIS-COMPARISON.md): Nemotron2/2 and Kimi2/2; the prior timeout did not recur under unchanged limits.

The locally validated [Planner decision correction PR395](https://github.com/lward27/pharness/pull/395) and [evaluator layer-reuse PR394](https://github.com/lward27/pharness/pull/394) are the next release inputs. Do not rerun d675159 merely to change a score. Freeze and release the corrected source before the affected Planner v2.6 checks. Local guards and small canaries do not close M04E's connected loop, M04F's full qualification or Finance acceptance. Sourcee508's original results remain unchanged; no local model, default switch or production approval was introduced.
