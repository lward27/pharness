# ASTRA M04: Coding reliability qualification evidence

Status: runtime repair released; offline qualification infrastructure verified;
**live model qualification and M04 acceptance remain open**.
Observed 2026-09-05. Authority: [M04](../../programs/autonomous-sdlc/ASTRA-04-CODING-RELIABILITY-QUALIFICATION.md).

## Released implementation

The earlier live evaluation, `infeval_01a070d9cc217571ae42e952c28c69c7`, did not
produce a qualifying report. Its evaluator exceeded the existing 1 GiB temporary
volume and was evicted. The persisted evaluation subsequently became failed.
This is a harness storage failure, not a measured model score; retain the
[failure evidence](ASTRA-M04-QUALIFICATION-STORAGE-FAILURE.json) and
[persisted result](ASTRA-M04-CODING-QUALIFICATION-RESULT.json).

The evaluator now removes each disposable fixture workspace after retaining its
intentional result artifacts. The repair preserves the frozen tasks, acceptance
checks, one-correction policy, time/token limits, 2 CPU/2 GiB evaluator limits,
4 GiB work volume and 1 GiB temporary volume. It does not add a coding backend.
The repair merged in [PR 327](https://github.com/lward27/pharness/pull/327), source
`fd740927110366a983de6bb0d3bc6c576577708b`.

All seven required images and the Linux AMD64 native bundle were built from that
source using the owner-authorized M1 Mac and the Rancher Desktop builder. Image
manifests, Linux AMD64 platform, OCI source/revision labels and native bundle
checksums were verified. The native bundle SHA-256 is
`7eb40b878af8d3a44d7af31407e85cd9d75a91dd7c2720fb2492f178c83e0f6e`.
[Build evidence](ASTRA-M04-RELEASE-BUILD.json) records all digests. This evidence
does not claim an SBOM, signature or cryptographically attested provenance.

Large registry layers hit the public upload limit. Publication completed through
the existing private registry write gateway using verified TLS and the configured
registry identity. No disabled TLS verification or alternative builder was used.
The dated [partial snapshot](ASTRA-M04-PARTIAL-RELEASE-IMAGES.json) records progress
before native packaging and release; the build and observation records supersede
its then-pending steps.

[PR 329](https://github.com/lward27/pharness/pull/329) updated only release image,
revision and derived execution-registry pins. It merged as
`548b978c33f8f32fb23d91120ef65a3502188d1c`. Helm lint/render, immutable release
checks and server-side validation of all 53 rendered resources passed before the
merge. [Premerge evidence](ASTRA-M04-RELEASE-PREMERGE.json) records the exact head.

At 11:41 UTC, Argo was Synced/Healthy at that release commit. All five continuously
deployed components had one ready, updated replica, the expected running image
digest and zero restarts. API/UI revisions matched the source. Finance generation
`dbgen_finance_20260827`, all 14 WorkItems, retained history and four retention
holds remained present. The release does not contain M05 migration 0052 or enable
Coding Reliability V2. See [observed release](ASTRA-M04-RELEASE-OBSERVED.json).

The namespace also retains 62 API eviction records from the previous release,
latest created on September 4 at 14:50 UTC. These are historical failed Pods, not
active replicas of this release. No current-release failed Pod was found and no
history was deleted. This point-in-time observation is not M12's unattended
reliability acceptance. The database-generation field `schema_version=0049`
records its initialization, not the current SQL migration count of 51.

## Packaged offline checks

The exact published evaluator ran without network, credentials or source mounts
under Linux AMD64 emulation on the authorized Mac. Its CPU/memory and temporary
directory ceilings remained bounded. `/work/artifacts` matched the cluster's
artifact configuration. Each suite ran two attempts:

| Frozen suite | Passed | Elapsed seconds |
| --- | --- | --- |
| Coding V2 | 48/48 | 258.30 |
| Repair V2 | 48/48 | 256.18 |
| Onboarding V2 | 24/24 | 6.31 |
| Planner V2 | 24/24 | 6.30 |
| Test diagnosis V2 | 24/24 | 6.26 |
| Verifier V2 | 48/48 | 12.23 |

All **216/216** checks passed. The [summary](ASTRA-M04-FD740-REPLAY-SUMMARY.json)
links each complete result. Successful completion with the same 1 GiB temporary
ceiling validates the repair against the prior storage failure. Docker tmpfs
uses memory; this is not a claim that the Mac and cluster have identical storage.

Two initial local wrapper mistakes are recorded separately: Docker's default
`noexec` temporary mount prevented fixture execution, and an omitted artifact
directory selected an unwritable default. Correcting the wrapper to provide
executable bounded scratch storage and the cluster's artifact path yielded the
results above. Neither setup attempt supplies a model score. The
[mount failure](ASTRA-M04-FD740-REPLAY-SETUP-FAILURE.json) and
[artifact-path failure](ASTRA-M04-FD740-REPLAY-ARTIFACT-DIRECTORY-SETUP-FAILURE.json)
remain in the evidence set.

## Live qualification still required

Fresh protocol calibration passed **30/30** at 11:42 UTC after verifying the
deployed source and aligned inference registry. See the
[calibration record](ASTRA-M04-FD740-GATEWAY-PREFLIGHT.json). The selected candidate remains
`builder-kimi-k2p7-code-v2@v1` through
`fireworks-kimi-k2p7-code@v1`. Its registry hash is
`sha256:0cd14616e7e2f3a0fda4e5b87a146e32b678e35c02d79c70d6296f40d01a9b02`.
Evaluation `infeval_01a07160294f7ce393d8a45d3ee23f7d` then started two attempts
on the exact runtime through Job `pharness-inference-eval-9459930dae6f`.
The [start receipt](ASTRA-M04-FD740-CODING-QUALIFICATION-START.json) pins its
suite, profile, prompt, policy, target and limits. The procedure records each response and does not automatically repeat an
uncertain request. Expired calibration from the previous runtime is not reused.

Remaining gates are two independent live frozen coding runs, two bounded repair
runs, all existing stage-specific suites (including each target's calibration), and the
existing disposable failure/repair WorkItem. Coding requires at least 21/24 first
pass and 23/24 after one correction, with each language at least 6/8 and 7/8,
zero hidden-test false passes and zero policy violations. A seeded repair fixture
is not proof that an actual Builder failure was repaired in a WorkItem.

Keep V2 and hosted creation disabled until their respective gates are satisfied.
Record live usage, elapsed time, failures and exact immutable profile bindings.
Do not replace missing live evidence with these deterministic replay results.

## Recovery and limits

This release retains schema-51 compatibility. M05's additive migration has a
different rollback floor and is not part of this rollout. Ordinary source work
was idle at the premerge check; no Finance application change, production approval
or deployment acceptance was generated by this release.

The Mac-backed builder depends on the Mac, Rancher Desktop, VPN and authenticated
forward remaining available. M02 proves the working path; M12 must prove its
operational behavior. A successful PHarness deployment is not autonomous SDLC
acceptance and does not close F13.
