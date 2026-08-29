# PHarness Stage-Aware Inference Gateway and Evaluation Milestone

## Status

Approved for implementation on 2026-08-29. The implementation baseline is
`5d7624d31d61759104ebb2902f128b1309535736`; the deployed application source
revision recorded at approval time is
`1c4b43478423bd835119c08501afab4de24967f8`.

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
