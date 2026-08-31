# Coding Reliability V2.2 release and deterministic replay evidence

Date: 2026-08-31  
Milestone status: active; provider-backed qualification is blocked externally  
Implementation PR: [#281](https://github.com/lward27/pharness/pull/281)  
Release-pin PR: [#282](https://github.com/lward27/pharness/pull/282)

## Immutable release identity

- Implementation revision: `1423a404d6e28fdcda48e8601332f509677c87a9`
- Release-pin revision: `1a1f57191932a8a1eee3a3904f42e8e4a8d4b5e1`
- Runtime: `sha256:82b07877b77eca9e15c7627b857f185e9c5ae1a4f930cd5f4dfef5b3aebd9db5`
- UI: `sha256:67a45095586d9a9f1249ae8f80018ac9be815f27d7b552f63cac4054225e3cff`
- Python runner: `sha256:95b103dcafe6bb3c0545c2c21c4f73546a03899f19d748aa07abcc2e20569da2`
- Node runner: `sha256:8789c0e0a18c04656a85f3236540eef6e56cf5c0eb1e84c7146d4dff0ca6d237`
- Model gateway: `sha256:6d260794765cf581bbdf9a5b6efb3b3f2be82e14a9c42bcefa988f3b2c986256`
- Evaluator: `sha256:ac6fc8104585779fb144b4cbfbd0002d9bbfe48243fbeff6aeab85161ba49ee7`

All six images were built on the dedicated `lucas-desktop` BuildKit worker from
the implementation revision. Each image was verified as `linux/amd64` with the
expected OCI source and revision labels.

## Release verification

- Argo Application `pharness` reached `Synced/Healthy` at the exact release-pin revision.
- API and UI both report implementation revision `1423a404d6e28fdcda48e8601332f509677c87a9`.
- Live API, UI, gateway, and proxy Pod image IDs equal their declared digest pins.
- API readiness reports database generation `dbgen_finance_20260827`, schema `0049`, operational mode `normal`, and platform revision alignment.
- API and gateway report the same inference registry hash: `sha256:0cd14616e7e2f3a0fda4e5b87a146e32b678e35c02d79c70d6296f40d01a9b02`.
- Gateway `/readyz` reported ready with 12 configured targets.
- `codingReliabilityV2.enabled` remains `false`; no unqualified policy was promoted.

## Deterministic gates

- `cargo fmt --check`: passed.
- `cargo test --workspace`: passed with `RUST_MIN_STACK=8388608`. One large
  existing async controller test exceeds Rust's default 2 MiB test-thread
  stack on Linux; this is a test-runner requirement, not a production stack
  failure.
- `cargo clippy --workspace --all-targets -- -D warnings`: passed.
- UI production build: passed.
- Vitest: 54 passed.
- Playwright: 85 passed and one intentionally skipped; the real API/controller
  Repo Mode journey passed.
- Helm lint, schema/render checks, server-side dry-run in namespace `pharness`,
  immutable-image scan, and OCI inspection: passed.

## Exact-image replay results

The digest-pinned evaluator image was executed natively on `lucas-desktop`.
These results prove deterministic harness behavior only; replay does not prove
model quality.

| Suite | Attempts/results | Result |
|---|---:|---|
| `coding-v2` | 48/48 | Two 24/24 attempts; each stack was 8/8; safety clean |
| `repair-v2` | 48/48 | Two 24/24 attempts; all corrections exercised; safety clean |
| `onboarding-v2` | 24/24 | Passed; safety clean |
| `planner-v2` | 24/24 | Passed; safety clean |
| `test-diagnosis-v2` | 24/24 | Passed; safety clean |
| `verifier-v2` | 48/48 | Passed; safety clean |

Every replay report identified runtime revision
`1423a404d6e28fdcda48e8601332f509677c87a9`.

## Provider qualification blocker

Fresh protocol verification
`inferverify_01a05871d4f17dd387cea031b2d42dfe` failed before semantic evaluation.
The gateway returned HTTP 412 with provider code `PRECONDITION_FAILED`. A
minimal credential-safe direct request confirmed that Fireworks account
`wardl1990` is suspended because of its billing/spend state.

This is an external provider-account failure, not a scored model or PHarness
coding failure. No semantic result was manufactured, no policy was promoted,
and the disposable supervised repair WorkItem was not started.

## Remaining acceptance sequence

After Fireworks billing is restored:

1. Run a fresh 30/30 Kimi K2.7 Code protocol calibration.
2. Run two provider-backed `coding-v2` attempts and require both 21/24 gates,
   per-stack floors, infrastructure validity, and zero safety violations.
3. Qualify Kimi K3 against `repair-v2` and require both 23/24 post-repair gates.
4. Run the remaining stage-specific candidate qualifications.
5. Enable V2 only for one disposable supervised WorkItem and exercise the
   deterministic failure/repair path.
6. Promote only exact policy revisions whose protocol and semantic gates pass.

