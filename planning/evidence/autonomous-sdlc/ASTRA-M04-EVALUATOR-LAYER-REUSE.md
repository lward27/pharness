# ASTRA M04: Reuse evaluator toolchain layers between releases

Status: implemented and validated locally; not deployed or included in source `d675159`'s release. This is a small release-preparation improvement, not a model, budget, suite or qualification change.

The evaluator Dockerfile put the changing release revision into its environment before installing packages and copying Rust/Codex toolchains. It also copied changing PHarness binaries before several stable toolchain files. Every release could therefore invalidate large layers even when those tools were unchanged. The complete preceding build spent 1,192 seconds in its reported layer-push phases; that observation motivates reducing avoidable rebuilds, but does not prove their contribution to upload delays.

The Dockerfile now prepares an inherited `runtime-tools` stage containing the existing packages, Rust/Codex files, non-root user and checks. The final stage adds the release metadata and two PHarness binaries. External base digests and the complete set of RUN, COPY, USER, WORKDIR and ENTRYPOINT instructions are unchanged. No package, compiler, sandbox, runtime provider or execution limit was added or changed.

## Validation

[The retained build and cache evidence](ASTRA-M04-EVALUATOR-LAYER-REUSE-VALIDATION.json) uses the selected Rancher Desktop `pharness-mac` builder and targets Linux AMD64. The ARM64 host cross-compiles as before; metadata describing that host is not the target architecture.

- The first toolchain-stage build passed in 66.307 seconds. Its existing checks ran as UID65532, verified the sandbox/code-mode binary and reported Cargo1.98.0, Rust1.98.0, Python3.11.2, Node24.20.0, npm11.19.0 and Git2.39.5.
- A second toolchain-stage build with a different release-revision argument passed in 0.431 seconds. All 12 non-base toolchain steps were cache hits.
- A complete evaluator-image solve passed in 0.482 seconds, reusing the same 12 toolchain steps and the unchanged source-d675159 compiler cache, then copying both application binaries into the final stage.
- Static comparison verifies identical external base digests and an identical multiset of all RUN/COPY/USER/WORKDIR/ENTRYPOINT instructions. Whitespace validation passes. No frozen fixture, dependency lock, gateway policy or model default changed.

These are three local cache-only builds: no registry push, cluster deployment or model call occurred. The second changes the revision argument, not application source bytes. The complete solve is not an uncached performance benchmark. Local build metadata is not a signed image attestation. This evidence does not measure registry push savings, prove a future release, or repair the separate intermittent gateway/registry timeout.

## Integration and recovery

Keep this preparation separate from the already-built source-d675159 release and its diagnostics. Merge with the next intentional source change before a later release freeze; the normal seven-image/native-bundle build, exact identity checks, observation and runtime qualification still apply. Do not rebuild or requalify the current release merely to include this optimization.

The change has no data migration. Reverting the Dockerfile restores the previous build layout; already-published artifacts remain immutable. Any eventual runtime rollback must still satisfy the program's compatible-reader and exact-policy qualification boundaries.
