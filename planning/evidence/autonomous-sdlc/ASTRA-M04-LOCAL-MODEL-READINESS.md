# ASTRA M04: Local-model gateway readiness and onboarding scope

2026-09-07. Implementation baseline: main `4206a1447744a08e3a2bf6712e0a028479a0fb23`. Prepared in a fresh `codex/astra-local-model-readiness` worktree. Status: implementation and local checks complete; immutable runtime release, real Minisforum connection and live model qualification remain separate gates.

## What changed

- The gateway deployment accepts additional **Secret references** through `inferenceGateway.additionalCredentials`. It retains the existing Fireworks mapping and mounts each extra credential only in the gateway. Schema/render checks reject reserved bindings, invalid path-like names and incomplete references. Merely adding a credential does not enable a model target or network path.
- Upstream attempts now honor the smaller of the target and the selected stage policy's limits. Previously a stage requesting one attempt could receive three. No budget was increased.
- Targets without `stream_options` support no longer receive that field. Local backends do not receive OpenRouter-specific structured reasoning fields. System messages, ordinary tool history and supported reasoning content retain their existing handling.
- Successful upstream responses must actually declare an event stream. An HTML/JSON success page is no longer relabeled as SSE. Error-body reads stop at 4 KiB or the existing stream-idle deadline; a server that sends error headers and hangs cannot occupy the gateway indefinitely.
- The shared Onboarding prompt revision is now `2026-09-07.1`. It distinguishes the current onboarding Run's configuration-change permissions from the proposed RepositoryContract's scope for future development. It grants no new write authority.

No model target, policy default, tool permission, token/turn/deadline budget, database migration, production approval or rollback boundary changed. Existing immutable image pins remain in this implementation change. The currently observed gateway is still source `92f8f1b`, digest `sha256:893e4a96c7afa22ae5e90ec1b1ae7a82874d4c7fc0a19df7e5009539445cad27`, Ready 1/1, with only the Fireworks credential mounted.

## Last M04 failure: observed fact and interpretation

The [original retained diagnostic](ASTRA-M04-92F8F1B-ONBOARDING-PRIMARY-RECOVERED.json), evaluation `infeval_01a07730b1e275e096ba85c8afb04d4c`, proposed these future `writable_paths`: `.pharness/repository.yaml`, `.pharness/instructions.md`, `.pharness/project.yaml`. Its explanation also asserted that only `.pharness/*` was writable. The public context specified onboarding artifact permissions, while the requested contract describes later development. The grader rejected `undeclared_onboarding_write_scope`.

The overlap in terminology is a plausible cause of the model's mistake, not proof of causation or a general capability verdict. The correction clarifies the shared production/evaluation prompt. The scorer, fixture inputs/identities and frozen suite thresholds are unchanged. A new regression verifies that a valid controller-bound proposal passes and the actual failed scope still fails. Historical results are unchanged and remain failed. A prompt change changes its recorded revision/hash and needs fresh live evidence; old qualification cannot be transferred.

## Validation and limits

[Validation record](ASTRA-M04-LOCAL-MODEL-VALIDATION.json) owns command outcomes and file/log hashes. Gateway tests use a loopback HTTP fixture through the actual grant handler and upstream client: authentication replacement, exact model mapping, fragmented SSE tool arguments, continuation with tool results, nonce replay rejection, one/two/three-attempt limits, missing credentials, wrong content type, and stalled responses. They are transport tests, not an LM Studio or coding-reliability pass. Existing exact-private-address registry validation remains enforced in production.

The explicit Helm tests parse real rendered resources and verify the extra Secret is mounted only in the gateway, the Fireworks mount survives, all other resources including NetworkPolicies remain identical, and invalid references fail. They are opt-in deployment tests because they require Helm; both are run explicitly for this slice. Initial rendering exposed the missing schema property; it was added before acceptance. The first new scorer regression used identity-free agent arguments rather than the controller-bound document; its test setup was corrected without changing production validation. All 47 existing evaluator tests passed in that run, and the corrected new regression passed separately.

The [maintained operator readiness check](ASTRA-M04-LOCAL-MODEL-PLATFORM-READINESS.json) passes for `lucas_engineering`; it observed Argo Synced/Healthy at source `4206a14`. It does not establish local-server connectivity. Windows instructions are source-reviewed, not executed on the user's new machine.

## Local target activation procedure

Use the [Windows setup guide](../../operations/ASTRA-MINISFORUM-WINDOWS-LOCAL-MODELS.md). The owner confirmed Windows 11 and 128 GB RAM. Reserved IPv4, loaded API model identifier, exact GGUF files/revision, server/runtime version and measured context capacity are pending. No guessed address was enabled.

Once those facts are available:

1. Complete the normal immutable PHarness release from one merged source: all seven images plus the native bundle, matching image pins, the actual `values-yfinance-production.yaml` overlay, observed Argo revision and live identities. This source introduces no schema migration. Keep the schema-55 recovery floor (`92f8f1b` or a compatible newer reader).
2. In the authoritative PHarness Helm source, add a **new target and policy revision**, preserving all current defaults. Match the model's real context and capability limits; preserve the selected diagnostic's input/output limits. Use `lm_studio`, the exact private `/32` and TCP port with the existing explicit HTTP opt-in, and an authentication binding. Do not enable a 32K target for a 65K-input policy.
3. Create/update `pharness/pharness-lm-studio`, key `api-key`, using the existing private Secret process. Add `inferenceGateway.additionalCredentials.lm-studio-api-key: {secretName: pharness-lm-studio, secretKey: api-key}` in the same activation change. The registry target must actually use the mapping: the gateway deliberately rejects unused credential mappings at startup. Never print the token or put it in Git.
4. Verify Windows Firewall source addresses, then render/review the target's exact egress rule and gateway-only mount. No direct worker-to-Minisforum access, browser CORS, LM Studio MCP execution, public ingress or provider fallback is needed.
5. Exercise authenticated network access, streaming/tool continuation, auth failure, model-unloaded failure and bounded timeout through the actual gateway. Record the loaded file identity; an API alias alone does not prove immutable weights.
6. Run one bounded stage diagnostic using the existing API/`lucas-ops` path. Retain public input/tool/schema hashes and settings. Only add a matched control when its common-input requirements hold. Do not replace or retroactively qualify the hosted baseline.

The independent next M04 action is a new `python-contract` canary on the released clarified prompt, using the existing MiniMax policy and unchanged limits. Inspect its result before a control or another case. Real local inference, M04D/E/F, autonomous Finance delivery, and M11 production approvals remain open.

Recovery: a failed local-server test leaves current workflow defaults in place. Retire a failing candidate from new selection while retaining its immutable revision/history and any configuration still needed by in-flight work. Recover the gateway through a compatible GitOps correction; never roll the schema-55 database back to an older reader or claim that restored availability turns a failed evaluation into success.
