# ASTRA M04: Accounted model turns and Mac build recovery

2026-09-07. Base: `bf2287e80664db84718da66b896b861c7ae9bfcb`, the merge of [PR 374](https://github.com/lward27/pharness/pull/374). This follow-up is required before releasing that preparation. M04 remains unqualified.

## Accounting correction

The real OpenAI-compatible and gateway clients could previously return an executable action with missing token usage. The runtime only increments consumption when usage exists. A server omitting usage could therefore allow work to continue without a meaningful token budget.

Both clients now require nonzero input and output counts, checked addition without overflow, and a total at least their sum before accepting an action. Missing, zero or inconsistent accounting is an unsupported-capability failure, not a model protocol error; it does not request an unmetered protocol correction. This stops further action but cannot recover the unknown tokens already consumed upstream. No fabricated usage, estimated accounting, model fallback or budget increase is introduced. Low-level protocol fixtures and the legacy Fireworks decoder retain their existing semantics.

[Validation](ASTRA-M04-METERED-MODEL-VALIDATION.json): 73 component tests pass, Clippy passes with warnings denied, and architecture checks pass including five checker regressions. Coverage includes absent/empty usage, each zero component, undercount, overflow, exact and conservative totals, and precedence over malformed action correction. The initial positive fixture omitted the action's required reason field; only that fixture was corrected. Two unrelated Helm tests remain explicitly ignored in this component command; their previous explicit passing run belongs to PR 374.

The Windows guide now requires streamed usage accounting and strictly checks the Boolean smoke-test argument. It remains source-reviewed, not executed on the owner's machine.

## Build findings and bounded repairs

The owner confirmed lucas-desktop is powered off and previously authorized this M1 Mac. The first release attempt from `bf2287e` is incomplete and will not be deployed. Five registry-verified images exist: runtime, UI, Python runner, Node runner and model gateway. Eval runner, Codex host and the native bundle are not accepted for that source. Do not mix those images into a later-source release.

The local Rancher Desktop VM had insufficient effective memory/CPU and a full 100 GiB disk. Its preserved VM now has 8 GiB RAM, four CPUs and a 160 GiB disk; the existing private BuildKit container has a 6 GiB memory ceiling and four CPUs. No persistent volume was erased. The public registry upload rejected a large layer with HTTP 413. The existing private registry route succeeds while retaining the canonical hostname, private CA and authenticated connection. No TLS verification was disabled.

The private BuildKit server then failed the actual evaluation-image `rustc --version` check with QEMU signal 11 / exit 139. The same pinned AMD64 Rust image ran successfully through Rancher Desktop. BuildKit v0.26.2 [selects its bundled emulator after architecture detection fails](https://github.com/moby/buildkit/blob/v0.26.2/solver/llbsolver/ops/exec_binfmt.go); its architecture check is known to [crash under some Rosetta configurations](https://github.com/moby/buildkit/issues/7052). The latter is supporting context, not proof of the exact local detector failure.

On the existing local `astra-tekton-buildkit` container only, `/usr/bin/buildkit-qemu-x86_64` was renamed to `/usr/bin/buildkit-qemu-x86_64.disabled-rosetta`, allowing the VM's already registered Rosetta interpreter to execute AMD64 binaries. The renamed binary is retained for reversal. This changes the executor, not the runtime checks. A fresh uncached build using the exact evaluation Rust image, UID 65532, verified x86_64, ran Cargo and Rust, compiled a program and executed it successfully. This proves that bounded probe, not the complete release. The container modification survives restart but must be deliberately reapplied and revalidated if the container is recreated.

The selected buildx client is `pharness-mac`, endpoint `tcp://127.0.0.1:12344`, with the existing mTLS client/server identity. Docker context is explicitly `rancher-desktop`. No global default builder or cluster endpoint changed. Stop before recreation/reversal while a build is active. To reverse the executor selection when idle, restore the retained binary to its original name; then repeat the real Rust check before building. Never accept the basic uname probe alone as proof that Rust works.

Six Dockerfiles now install the unchanged cross compiler/toolchain before copying application source and setting the release identity. Source changes therefore preserve that compiler layer. Final image stages, pinned bases, package sets and required runtime checks are unchanged. No shared mutable Cargo cache was added. Actual image builds remain the acceptance gate for these Dockerfile edits.

## Resume and evidence

Private attempt logs and recovery records are retained under `/Users/wardl/Personal/apps/pharness-release-artifacts/bf2287e80664db84718da66b896b861c7ae9bfcb/`. Do not commit unfiltered registry upload URLs: query strings can contain opaque upload state. The build failure does not justify changing model budgets or M04 thresholds.

After merging this follow-up, build all seven images and the native bundle from one new merged revision, validate the actual production overlay, then release through GitOps. Preserve the schema-55 recovery floor. Verify the deployment before one unchanged-policy MiniMax `python-contract` canary. The live source remains `92f8f1b` at this checkpoint. The owner is still configuring Windows LM Studio; no endpoint, model identifier, credential, target or policy default was invented or enabled.
