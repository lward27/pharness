# PHarness

PHarness is an SDLC control plane embedded in the environment that hosts a
product. Its approved direction is one bounded request progressing through
coding, tests, source delivery, immutable builds, staging, human production
approval, and runtime verification in `lucas_engineering`.

**Current reality:** the cluster runtime, product/repository registration,
bounded coding machinery, source-only WorkItems, typed delivery capabilities,
and Lamina operator console exist. They do not yet establish one accepted,
fully autonomous hosted journey. The existing gateway coding path still needs
to pass its frozen reliability qualification. The
[ASTRA program](planning/programs/autonomous-sdlc/ASTRA-00-PROGRAM.md) implements
and proves that connection; its status must not be confused with deployed behavior.

## Get started

Start with the [documentation map](planning/README.md), then the
[current baseline](planning/evidence/assessments/ASTRA-CURRENT-BASELINE-ADDENDUM.md).
The [product vision](planning/design/product-vision-and-boundaries.md) explains
the approved boundary; the [architecture map](planning/architecture/README.md)
explains the existing implementation.

The operator works from a Product and a bounded WorkItem. Source merge,
deployment, and verified runtime behavior are distinct results. Legacy source-only
success remains valid history and does not imply that anything was deployed.

## Develop

The workspace contains the Rust runtime, API, store, workers, gateway, evaluator,
and CLI, with the React/Vite console under `ui/`. Local runtime configuration is
illustrated in [config/pharness.example.toml](config/pharness.example.toml).
Local execution remains useful for development; it is not the primary product promise.

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

Use the [architecture guardrails](scripts/check-app-module-boundaries.sh) and
[UI guidance](ui/AGENTS.md) when changing those boundaries. Run the relevant UI
checks declared in [ui/package.json](ui/package.json). Fixes must preserve the
single-writer database and scoped reader/writer/effect responsibilities.

## Operate and release

Use [operational runbooks](planning/operations/README.md) and the
[latest baseline evidence](planning/evidence/assessments/ASTRA-CURRENT-BASELINE-ADDENDUM.md).
Manage deployments through their GitOps sources. Never infer release acceptance
from readiness or Argo health alone.

The current PHarness release procedure builds **seven** images from one exact
merged source revision: runtime, UI, Python runner, Node runner, model gateway,
evaluation runner, and the existing Codex-host image. Preserve the native host
bundle required by the build even though expanding that backend is deferred.
The [build entry point](scripts/pharness-build.sh) requires an explicit builder;
[release pinning](scripts/pharness-release-pin.sh) takes the same source revision
and all seven digests. It validates a separate clean GitOps release change.
The scripts do not replace merge authorization or live release verification.

For this program, local builds use the selected Rancher Desktop builder and
its uncached Linux AMD64 execution check. A digest identifies an artifact;
it is not by itself an SBOM, signature, or vulnerability attestation.

## Reliability and roadmap

[ASTRA M04](planning/programs/autonomous-sdlc/ASTRA-04-CODING-RELIABILITY-QUALIFICATION.md)
owns the current gateway qualification gate: the frozen 24-task suite, two
independent qualifying runs, per-stack thresholds, protocol/stage checks,
and no hidden-test false passes or policy violations. Deterministic replay
belongs in CI. Live provider qualification is an explicitly authorized,
bounded operation and must not expose credentials or run as ordinary CI.

The [numbered program](planning/programs/autonomous-sdlc/ASTRA-00-PROGRAM.md)
owns implementation order and F01–F16 review coverage. Repo Mode is being retired
as a separate experience, not deleted as useful implementation. Generic platform
adapters, multi-repository orchestration, incident initiation, new navigation,
and additional coding backends remain deferred.

## Evidence and history

[Dated evidence](planning/evidence/README.md) states what was actually observed.
[Implemented records](planning/implemented/README.md) explain shipped decisions.
[Archives](planning/archive/README.md) and historical source-only contracts retain
prior boundaries; they do not authorize new work or override the ASTRA program.
