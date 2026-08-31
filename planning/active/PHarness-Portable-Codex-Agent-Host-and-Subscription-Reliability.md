# PHarness Portable Codex Agent Host and Subscription-Backed Reliability Milestone

## Summary

Add Codex as a first-class PHarness agent execution backend for Planner,
Builder, Repair, and Verifier while retaining controller-owned deterministic
Test. Codex is controlled through the official App Server protocol and remains
separate from the OpenAI-compatible inference gateway.

The first host is `lucas-desktop`, using a native systemd host service and
rootless Podman. The host protocol, binary, runner images, and configuration
must remain portable to a Minisforum replacement and to a disabled-by-default
Kubernetes deployment. WorkItems bind logical host pools and immutable
execution policies, never physical hostnames.

Initial qualified policies use GPT-5.6 Sol: high reasoning for Planner and
Builder, xhigh for Repair and Verifier. A trusted standalone host may use an
interactive ChatGPT subscription session. Kubernetes must use an explicitly
supported noninteractive credential and must reject a mounted ChatGPT session.

Implementation baseline: `1c20ec1bcfbb36281cdf5e148843b6c68003ec8c`.

## Implementation Changes

### Execution policy and provenance

- Introduce `StageExecutionDriver` with `pharness_runhost` and
  `codex_app_server` variants.
- Persist immutable `AgentExecutionPolicyRevision` and
  `ResolvedAgentExecutionBinding` records containing the exact driver, Codex
  version, model, effort, prompt/output-schema hashes, runner digest, host pool,
  authentication class, limits, and configuration hash.
- Preserve existing inference bindings and legacy Runs. Codex is not an
  inference target and never routes through `pharness-model-gateway`.
- WorkItem creation may pin Planner policy; stage-chain authorization pins
  Builder, Repair, and Verifier policies. Clients select only active qualified
  policy IDs and cannot provide hosts, images, models, URLs, or credentials.

### Portable outbound host

- Add `pharness-codex-host` with durable AgentHost, capability snapshot, and
  AgentLease state.
- Use one-time 15-minute enrollment, 10-second heartbeat, 45-second lease
  expiry, transactional claims, and API-owned durable writes.
- Reuse immutable attempt-context, event, preparation, control, artifact, and
  outcome endpoints from the existing worker contract.
- A lost heartbeat pauses the Run and preserves its sticky workspace. Only the
  same host may resume it; permanent loss requires an explicit abandon and a
  new correction or replan workspace.

### Workspaces and execution

- Planner uses a temporary read-only checkout. Builder, deterministic Test,
  optional Repair, second Test, and Verifier remain bound to one host and one
  durable workspace.
- Run the standalone host as an unprivileged systemd service and use rootless
  Podman to launch exact digest-pinned EnvironmentProfile images.
- Extend Python, Node, and evaluation runners with the pinned Codex App Server
  entrypoint while preserving current worker entrypoints.
- Source checkout occurs host-side with anonymous or reader-only credentials;
  credentials never enter the Codex container. Context repositories and Git
  metadata are read-only. Builder/Repair receive only contract-declared
  writable paths; Verifier is fully read-only.
- Start one App Server thread per StageExecution, allow one infrastructure
  restart/resume, disable web search/connectors/plugins/subagents/cloud handoff,
  disable command network, and require typed terminal output.
- Keep ChatGPT auth outside repository roots, mount it read-only for App Server,
  deny command-sandbox reads, and block qualification if that boundary cannot
  be proven.

### Stage and evidence behavior

- Planner emits the existing proposed WorkPlan without mutation.
- Builder and Repair may edit only declared paths and run offline profile tools;
  they cannot install packages, mutate Git, push, or create PRs.
- Test executes exact RepositoryContract acceptance commands without a model.
  One repair is permitted; a second code failure blocks.
- Verifier reviews intent, plan, diff, acceptance, documentation, and risks from
  a read-only workspace.
- Before sealing changes, validate source baseline, Git metadata, path scope,
  symlinks, introduced files, diff hash, and status hash. Upload the existing
  workspace diff/status/acceptance evidence for central ChangeSet delivery.
- Subscription quota exhaustion pauses honestly and never triggers fallback.

### APIs, UI, and packaging

- Add operator APIs for execution policies, agent hosts, enrollments, and
  state-hashed host actions; add internal enrollment/heartbeat/claim/completion
  APIs.
- Extend WorkItem, chain authorization, Run, StageExecution, StageOutcome,
  summary, and evidence models with sanitized driver/host provenance.
- Add Settings > Agent Hosts and stage policy selectors in WorkItem Advanced;
  show exact backend, model, host pool, workspace, thread/resume, deterministic
  test, correction lineage, and blocker state.
- Produce a checked Linux AMD64 native/systemd bundle, a digest-pinned host
  image, Codex-enabled runners, and disabled-by-default Helm StatefulSets with
  sticky PVCs. Helm must reject ChatGPT-session authentication in Kubernetes.

## Qualification and Rollout

- Cover enrollment, leases, host loss/resume, cancellation, sandbox boundaries,
  runner matching, structured App Server protocol, deterministic Test, evidence
  equivalence, legacy compatibility, systemd, Podman, and Kubernetes rendering.
- Require 30/30 protocol calibration cases before semantic evaluation.
- Run the frozen 24-task Rust/Python/Node suite twice through the exact Codex
  policies. Require at least 21/24 first-pass and 23/24 post-repair successes,
  stack minimums, zero hidden-test false passes, zero policy violations, and the
  existing Verifier false-approval/rejection gates.
- Build every artifact from one merged SHA on `lucas-desktop`, pin digests in a
  separate release commit, deploy centrally with the feature disabled, install
  and enroll the host, qualify exact policies, then enable new selection.
- Live acceptance is Finance FRC-2: a typed `GET /markets/US` frontend adapter
  and bounded Market Overview panel against yfinance merge
  `12ff05dab47778dd2344970001c4218c1825db96`, with `test`, `lint`, and `build`
  acceptance, one optional repair, manual PR merge, and observed source closure.
- Migration to the Minisforum enrolls it into the same pool, verifies identical
  digests, prioritizes it for new leases, drains `lucas-desktop`, and never moves
  an active workspace.

## Boundaries

- ChatGPT subscription execution is a trusted single-user experiment subject to
  OpenAI limits; it is not exposed as a shared HTTP service.
- Onboarding remains on the existing backend. Test remains deterministic.
- No automatic fallback, merge, mid-chain migration, Kubernetes live activation,
  autoscaling, distributed workspace, local-model qualification, Connected Mode,
  or deployment expansion is included.
