# PHarness Stage-Aware Inference Gateway and Evaluation Milestone

## Status

Implemented and accepted on 2026-08-30. The implementation baseline was
`5d7624d31d61759104ebb2902f128b1309535736`; the accepted application source
revision is `117feed8cc2b030bc66ab84a3f1bcfc5b4207b47`, and the accepted
digest-pinning release commit is `02e87a651c5ce7fb6c3c3453d382ba48e6825024`.

## Objective

Introduce an in-cluster `pharness-model-gateway` and immutable stage-specific
inference policies. Workers must reach Fireworks, OpenRouter, LM Studio,
llama.cpp, and future OpenAI-compatible targets only through the gateway. The
gateway alone holds upstream credentials. Planner, Builder, Tester, Verifier,
and onboarding proposer executions pin qualified policies containing exact
target, model, reasoning, context, generation, tool, and retry behavior.

## Locked decisions

- GitOps owns target revisions, policy revisions, defaults, credentials, and
  network routes.
- Operators receive qualified defaults and may select another qualified policy
  through Advanced controls; they never provide arbitrary endpoints or knobs.
- OpenRouter targets pin one provider endpoint, require all parameters, and
  disable provider fallback.
- Live milestone acceptance proves Fireworks parity through the gateway.
  OpenRouter, LM Studio, and llama.cpp receive complete deterministic adapter
  coverage but are not live acceptance dependencies.
- Direct Fireworks remains the rollback path for new Runs until gateway parity
  passes. Gateway-bound Runs are never silently rerouted.
- V1 uses streaming OpenAI-compatible Chat Completions, native function tools,
  one PHarness action per turn, and one gateway replica.

## Deliverables

1. Provider-neutral request/SSE/tool/reasoning normalization and a dedicated
   non-root gateway image and Deployment.
2. Immutable `InferenceTargetRevision`, `StageInferencePolicyRevision`,
   `ResolvedInferenceBinding`, stage selections, target verifications, and
   policy qualifications.
3. Sixty-second, request-hash-bound, single-use HMAC grants for every model
   turn, plus worker/API credential removal in gateway mode.
4. Target/policy APIs, onboarding/WorkItem/stage-chain selection, exact resume
   binding, sanitized readiness/provenance, and Settings/WorkItem UI controls.
5. Stage-specific onboarding, Planner, Builder, Tester, and Verifier evaluation
   suites. Candidate Fireworks reasoning policies are promoted independently
   only when their gates pass.
6. Immutable build and Argo rollout of API, UI, Python runner, Node runner, and
   gateway from one merged SHA on `lucas-desktop`, with direct-versus-gateway
   Fireworks parity evidence and a documented rollback.

## Acceptance boundary

No automatic provider failover, cost routing, speculative execution,
provider-managed tools, Responses API, embeddings, model lifecycle management,
or live OpenRouter/local-model qualification is included. Failed policy
candidates remain evidence and leave the accepted legacy policy active.

## Acceptance evidence

- Argo CD reached `Synced/Healthy` at the exact release commit. API and UI both
  reported `117feed8cc2b030bc66ab84a3f1bcfc5b4207b47`, live Pod image IDs
  matched the declared runtime, UI, and gateway digests, and database generation
  `dbgen_finance_20260827` remained unchanged.
- All six images were built on `lucas-desktop` from the accepted source SHA and
  independently verified as `linux/amd64` with matching OCI revision labels:
  runtime `sha256:371eb02971f84a93ced446619160ac17e6938ce88e916974a09b8022347cf63c`,
  UI `sha256:2597f1b24b1d357799206bdd77b4c803a7868e3bfb8c756c578a613d2011a370`,
  Python runner `sha256:0cff4a037136a25779b9d9f77650858bc2b55e4fd7ec0d82412ea39d41fb504e`,
  Node runner `sha256:ab44bcfeb08468775e2c43d739bea1fb1514113fc7a1bc3b78788b38e2b5ba9f`,
  gateway `sha256:0af0c5508b6b7177d9fde4d9f39e64cceafc8dc408823aabf4ae033d1bc8cb0e`,
  and evaluation runner `sha256:828326a5685014ac7bb3aad7a720ecb970be5438871a028c00d20a8f8bd4fc22`.
- Direct-versus-gateway matched Fireworks evaluation passed its parity gate.
  Direct execution produced 6 fixture passes and 15 acceptance passes; gateway
  execution produced 7 fixture passes and 16 acceptance passes, with no safety,
  environment-discovery, or context-limit regression.
- The onboarding, Planner, Builder, Tester, and Verifier candidate policy suites
  were executed. Every non-Kimi candidate failed its stage qualification gate,
  remained durable evidence, and was not promoted. The accepted defaults stayed
  on the legacy policy except for the previously qualified Kimi Tester-low and
  Verifier-high policies.
- An unqualified Qwen Planner policy was rejected before WorkItem creation with
  no fallback. The live mixed-policy WorkItem
  `witem_01a0531881897aa18ead92fcf4d864b7` then pinned legacy Builder,
  Kimi Tester-low, and Kimi Verifier-high. Its Builder resumed in place after
  one exact budget extension, and Implement, Test, and Verify all sealed as
  succeeded. The WorkItem stopped at the proposed ChangeSet review boundary;
  no source mutation was performed.
- The live exercise exposed an invalid Planner acceptance-name defect. PR #264
  added controller enforcement for exact declared acceptance names and complete
  coverage. The deployed controller rejected the invalid replacement plan with
  the exact stop reason, and a durable operator annotation produced a valid
  revision without bypassing the guard.
- Required gates passed: `cargo fmt --check`, `cargo test --workspace`,
  workspace/all-target Clippy with warnings denied, UI production build, 52
  Vitest tests, 85 Playwright tests with one intentional mobile skip, Helm lint,
  immutable-image render scan, a 51-object Kubernetes server-side dry-run, and
  Linux/AMD64 OCI inspection. The desktop real-server Repo Mode browser journey
  passed; live mixed-policy actions were exercised through the real API because
  the in-app browser's localhost URL policy prevented that one interactive path.
- Direct Fireworks remains enabled as the explicit rollback path. API and gateway
  registry hashes match, and no provider or policy fallback is enabled.
