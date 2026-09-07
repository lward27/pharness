# ASTRA M04: Metered gateway release

Compiled source: `81edd9e81d280b0eda866b719c86562b3ff2f02c` (PRs [374](https://github.com/lward27/pharness/pull/374) and [375](https://github.com/lward27/pharness/pull/375)). Status at pin preparation: all artifacts and deployment preflight pass; rollout and live diagnostic pending.

The release includes gateway local-server preparation, truthful usage requirements and the clarified Onboarding scope prompt. It enables no local target, default-model change, budget increase, new schema or Finance production approval.

[All seven build receipts and native bundle](ASTRA-METERED-RELEASE-BUILD.json) share the one merged source. [Independent registry verification](ASTRA-METERED-RELEASE-ARTIFACTS-VERIFIED.json) validates manifest/config hashes, Linux AMD64 and source labels, plus the native archive checksum. The official packaging procedure also passed all 13 internal checksums, AMD64 executable checks and bundle runtime checks. SBOMs, signatures and cryptographically verified attestations are not established by these receipts.

The [Mac builder record](ASTRA-METERED-MAC-BUILDER.json) and [recovery analysis](ASTRA-M04-METERED-MODEL-TURNS.md) explain the selected executor while lucas-desktop is off. The actual evaluation image ran Cargo, Rust, Python, Node, npm and Git as UID 65532; the original Rust crash did not recur. Full build took approximately 38 minutes, including slow registry uploads. The incomplete `bf2287e` artifact set was not deployed or mixed into this release.

[Deployment preflight](ASTRA-METERED-RELEASE-PREFLIGHT.json) uses the actual production values file, immutable images and a passing server-side dry-run. Agent execution policy changes are restricted to runner image bindings and their hashes. [The five-minute baseline](ASTRA-METERED-BASELINE-SERVICE-WINDOW.json) has 11 healthy console/API samples, fresh availability/restart telemetry and required log delivery with no severe-log matches. It is an internal service baseline, not public ingress acceptance or coding qualification.

Observe the exact Argo pin revision and Pod image identities after merge, then complete a ten-minute service window. Keep the schema-55 compatible reader floor (`92f8f1b` or a compatible newer release); this release makes no schema change. A rollback, if needed, must be a compatible GitOps pin correction. Do not reset the database or infer that service recovery makes an evaluation pass.

Then run one unchanged-policy MiniMax Onboarding `python-contract` diagnostic through the maintained operator interface and inspect its native result. Preserve failed historical evidence. The next result must not receive a qualification merely because it is a successful diagnostic. Real Minisforum connectivity awaits the owner's Windows setup and exact endpoint/model facts; the [setup guide](../../operations/ASTRA-MINISFORUM-WINDOWS-LOCAL-MODELS.md) is ready.
