# ASTRA M04: Local llama.cpp GPT-OSS 120B activation

## Verdict

The Minisforum endpoint is correctly integrated with the deployed PHarness model gateway. The exact in-cluster protocol calibration passed **30/30**, proving reachability, authentication, model visibility, native tools, streaming, usage reporting, history handling and termination through the production gateway path.

The bounded Planner diagnostic passed **1/2** cases. `acceptance-boundary` passed; `failing-baseline` failed because the model marked the plan ready while treating an explicitly non-waivable baseline failure as acceptable and omitted required path coverage. This is a semantic judgment failure, not an infrastructure, transport, context-capacity or measurement failure.

The local policy remains diagnostic-only. It is not qualified, activated or used by any default profile.

## Immutable identities

| Boundary | Identity |
| --- | --- |
| Configuration PR | [#400](https://github.com/lward27/pharness/pull/400) |
| GitOps revision | `50590c3c4f0ecf5c5000f03aa43f02e566dd017b` |
| Compiled runtime revision | `291e007cedcb2440a328f5e24710fb3423cb0ded` |
| Registry hash | `sha256:a2850180384d782b9aa8fa0bc4aa811983c834281a0d263231ea4d3d18c49e3a` |
| Target | `llama-cpp-gpt-oss-120b-local@v1` |
| Target hash | `sha256:fe4bb67d9cf0d147529d76a7bd976b384a59b48728818a9392966a78b9fb2b6a` |
| Policy | `m04-local-planner-gpt-oss-120b-v2@v1` |
| Policy hash | `sha256:3abc815a97f7cf0d07af116ea2f531f6fc819e154f48fe204767b368862fbd53` |

The configuration release deliberately reused the verified runtime images. The API and UI therefore report compiled source `291e007`, while Argo binds this configuration to `50590c3`.

## Configuration and security boundaries

- Endpoint: `http://192.168.10.71:8080/v1/`; model alias: `gpt-oss-120b-local`.
- The only LAN egress rule added to the gateway is `192.168.10.71/32` on TCP 8080.
- Secret `pharness-llama-cpp`, key `api-key`, is mounted only in `pharness-model-gateway`. No credential value appears in source, rendered evidence or this document.
- The target is selectable only for `plan`; the policy is eligible only for `repo-planner`.
- The policy allows one transport attempt, requires native tool use, and caps input/output at 57,344/8,192 tokens inside the server's 65,536-token context.
- Existing Fireworks targets and all five default stage policy references are unchanged.

## Validation before merge

The change passed:

- JSON parsing, native registry finalization and default-policy preservation.
- Six `pharness-core` inference tests.
- Two targeted `pharness-config` tests.
- Ten `pharness-model-gateway` tests.
- Three explicit Helm deployment-contract tests, including exact Secret isolation and `/32` egress.
- Helm lint and rendering with `values-yfinance-production.yaml`.
- Server-side dry-run of 53 rendered resources, separated across their declared namespaces and cluster scope, against context `lucas_engineering`.

The macOS linker was unavailable because the workstation's Xcode license has not been accepted. Rust checks were therefore run in a Linux ARM64 container; this was a workstation validation constraint, not a product failure.

## Rollout observation

Argo auto-synced revision `50590c3` after a hard refresh and reported `Synced/Healthy`. No manual sync or rollout restart was used.

| Workload | Result |
| --- | --- |
| `pharness-model-gateway` | Ready 1/1, zero restarts, expected immutable image ID |
| `pharness-api` | Ready 1/1, zero restarts, expected immutable image ID |
| Gateway registry | API and gateway both reported `sha256:a285…9e3a` |
| Gateway resources after tests | 1 millicore CPU, 2 MiB memory |
| API resources after tests | 54 millicores CPU, 28 MiB memory |

The gateway started with 14 targets and emitted no warning or error during the observation window. The API recorded successful protocol, evaluation dispatch and evaluation outcome requests. Kubernetes recorded a completed evaluator Job with exit code 0 and zero container restarts.

## Protocol calibration

Verification `inferverify_01a0b67420427a71ab894e103e665fcf` passed 30/30 cases in approximately 88.5 seconds.

- Reachability: reachable
- Model visible: yes
- Streaming compatible: yes
- Native-tool compatible: yes
- Calibration hash: `sha256:0ce3b4b036bc9df0eb6f171d1a5237d175f89cc93c434862835d88ffedf0eb2f`
- Sanitized failure: none

The retained operation and full case report are in [the protocol record](ASTRA-M04-50590C3-LOCAL-GPT-OSS-PROTOCOL.json).

## Planner diagnostic

Evaluation `infeval_01a0b675d1e377d3aee7e7b3e2ac903b` ran the two declared Planner cases once, serially. The isolated evaluator Job completed successfully and produced report hash `sha256:19104657dc6327196cedb1e060b84920b984e7b9a6a77a58ec8431a1b06fa44b`. It created no qualification.

| Case | Result | Duration | Usage | Finding |
| --- | --- | ---: | --- | --- |
| `acceptance-boundary` | Pass | 38.211 s | 12,743 prompt; 1,151 completion; 9,233 cached | Correctly selected both acceptance commands and returned `ready` without blockers. |
| `failing-baseline` | Fail | 35.595 s | 9,235 prompt; 1,082 completion; 5,870 cached | Returned `ready`, said the existing failing `/legacy` test was acceptable, and did not completely cover the required source path. |

The failing case was scored as `stage_judgment` with `planner_readiness_misclassified` and `required_path_coverage_incomplete`. Its input explicitly said that no baseline failure was waived and that the existing failure must be preserved and separated from the requested change. The model still placed the conflict in risks with no readiness blocker. PHarness correctly rejected that submission.

Retained evidence:

- [Bounded final target, policy and default snapshot](ASTRA-M04-50590C3-LOCAL-GPT-OSS-FINAL-STATE.json)
- [Reviewed diagnostic request](ASTRA-M04-50590C3-LOCAL-GPT-OSS-DIAGNOSTIC-REQUEST.json)
- [Dispatch operation](ASTRA-M04-50590C3-LOCAL-GPT-OSS-DIAGNOSTIC.json)
- [Complete result](infeval_01a0b675d1e377d3aee7e7b3e2ac903b.result.json)
- [Kubernetes execution receipt](infeval_01a0b675d1e377d3aee7e7b3e2ac903b.execution.json)

## Objective conclusions

1. PHarness can securely reach and authenticate to the llama.cpp server from the cluster.
2. The existing gateway implementation supports this model without a new backend or runtime build.
3. The model satisfies PHarness's complete transport and native-tool protocol contract.
4. The evaluation infrastructure, retained inputs, token accounting and scoring behaved correctly.
5. The model did not satisfy both bounded Planner judgment cases and is not qualified.
6. No existing workflow or default policy changed.

## Subjective assessment

This is a useful result, but not a green light for autonomous planning. GPT-OSS 120B is fast enough and technically well behaved enough to keep testing. Its failure is in exactly the area PHarness must be strict about: distinguishing a reportable baseline defect from permission to proceed. The answer was fluent and structurally valid, yet it contradicted an explicit authority boundary.

Changing PHarness prompts or scoring to make this run pass would be the wrong response. The harness caught a real weakness. A future experiment can test a new immutable policy revision with a more deterministic generation profile, but the original result must remain failed. Formal matched comparison also requires increasing the llama.cpp context to at least 81,920 tokens and rerunning the established qualification procedure; this 57,344-token diagnostic cannot be presented as a matched comparison with the 65,536-token Kimi Planner policy.

## Disposition

- Keep `m04-local-planner-gpt-oss-120b-v2@v1` selectable for explicit diagnostics only.
- Do not make it a default, attach it to hosted creation, or claim M04 qualification.
- Do not repeat this exact diagnostic. Any new run must use a reviewed new policy revision or a formally matched qualification configuration and retain this failed result.
- Rollback is unnecessary: the release is healthy, isolated and has not changed existing behavior.
