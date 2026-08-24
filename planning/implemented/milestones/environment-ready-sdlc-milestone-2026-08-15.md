# Environment-Ready Supervised SDLC Milestone Evidence

Date: 2026-08-15

## Implementation state

The implementation branch adds the strict `pharness.dev/v1alpha1` repository
contract, immutable environment profiles, a credential-isolated preparation
Job, signed environment snapshots, typed environment and acceptance tools,
resumable run budgets, attempt-scoped workspace grants, lifecycle review
actions, corrected operator evidence, and the corresponding operator UI.

The initial production runner is `python-3.11`, Linux AMD64 only. Preparation
and coding Jobs have no direct internet egress. They use separate HTTP CONNECT
proxies with server-owned exact host allowlists; the proxy implementation also
rejects non-CONNECT traffic and ports other than 443.

The implementation values intentionally keep the new runner profile inactive
and pinned to the historical runtime digest. A post-merge release commit must
replace the API, UI, and runner pins with artifacts built from one exact merged
source revision before the profile can be activated.

## Local validation

- Rust formatting, workspace tests, and workspace/all-target Clippy with
  warnings denied pass.
- The UI production build, Vitest suite, and all 46 desktop/mobile Playwright
  cases pass.
- Helm lint, schema validation, rendered `:latest` scans, Kustomize build, and
  Kubernetes server-side dry-runs pass against the approved cluster.
- Both the runtime and Python runner cross-build successfully as Linux AMD64
  images on the Apple Silicon development host.
- The runner smoke verifies the non-root identity, executable inventory,
  absence of Docker/Podman, writable virtualenv, and a complete yfinance
  `--require-hashes --only-binary=:all:` installation.
- The immutable build and release-pinning scripts require runtime, UI, and
  Python-runner artifacts from the same merged revision. Release pinning
  activates `python-3.11` only after recording its independently built digest.

## Matched Fireworks gate

The `coding-v1.7` suite retains eight fixtures and two attempts per fixture.
It replaces one non-differentiating configuration fixture with a prepared
Python fixture that supplies an EnvironmentSnapshot and ProjectContract and
records any coding-phase shell action as an environment probe.

An initial candidate scored 14/16 with zero safety or context failures and
passed both Python attempts in five turns with zero probes. Its two Rust misses
exposed a prompt contradiction for legacy development runs without a project
contract. Prompt version `2026-08-15.2` makes typed acceptance mandatory only
when a verified contract is present while preserving policy-gated local tests
for legacy development runs.

The matched reports use deployed runtime revision
`9ed987394aad0ba085d746287392f07c1eae7035`, Fireworks model
`accounts/fireworks/models/kimi-k2p6`, fixture revision `coding-v1.7`, prompt
version `2026-08-15.2`, temperature 0.1, 4,096 output tokens, 24 turns, and two
attempts per fixture.

```text
baseline_passes: 6
candidate_passes: 16
additional_passes: 10
baseline_context_failures: 0
candidate_context_failures: 0
candidate_safe: true
candidate_python_probe_free: true
gate_passed: true
```

The reports and fixture artifacts remain untracked under
`target/pharness-evals/`.

## Post-merge immutable build correction

The first three-artifact Tekton build from merge `e9e5212` produced the UI
image but stopped both Rust-based images before compilation because Kaniko did
not synthesize BuildKit's `TARGETARCH` argument. The failed runtime and runner
PipelineRuns were retained as evidence and were not blindly retried. The build
workflow now explicitly selects `linux/amd64`, passes `TARGETARCH=amd64`, uses
the approved `lucas_engineering` context on every Kubernetes call, and exits
promptly when Tekton reports a terminal failure. All three artifacts must be
rebuilt from the merge that contains this correction; the earlier UI digest is
not release-eligible.

## Remaining supervised boundaries

Deployment cannot start from the implementation branch. The required order is:

1. Manually merge the implementation PR.
2. Build and publish API/runtime, UI, and Python runner from that exact merge
   SHA.
3. Manually merge a separate digest-pinning release commit and let the
   PHarness Argo Application deploy it.
4. Manually merge the yfinance onboarding PR from its isolated worktree.
5. Submit a fresh WorkItem and complete the source PR, Tekton build, GitOps PR,
   explicit Argo sync, verification, and rollback-readiness boundaries through
   the operator console.

The two historical failed yfinance WorkItems remain untouched.
