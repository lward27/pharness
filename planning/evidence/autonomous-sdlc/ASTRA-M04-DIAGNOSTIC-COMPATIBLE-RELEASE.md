# ASTRA M04: Compatible diagnostic release

Prepared: 2026-09-06T13:49:54.744269+00:00. Source: `92f8f1b8e98dd45d0a01e030aeb99ef9bcf95267` (merged PR 371).
Status: complete immutable artifacts and reviewed release pins; live rollout, live database preservation and model diagnostics are pending at this commit.

## Released behavior and source boundary

This candidate contains the approved M04A trusted submissions, M04B versioned context delivery, M04C source-backed stage measurements, and M04D bounded diagnostic execution with recorded common-input controls. Native code owns original discovery identity; partial diagnostics cannot create qualifications or activate policies. Existing models, defaults, execution budgets and the one-correction limit remain intact.

The source matches the tested `f0c7afd` implementation across crates, dependency locks, UI and Docker build files. Its [627 component checks](ASTRA-M04-DIAGNOSTIC-CANARIES-VALIDATION.json) are engineering evidence; live model behavior remains unproven. M08/M09 integration drafts are not part of this candidate.

## Immutable build and release review

[Build evidence](ASTRA-M04-DIAGNOSTIC-RELEASE-BUILD.json) links all seven image digests to this source. The existing `lucas-desktop` BuildKit worker built Linux AMD64 artifacts through the Rancher Desktop client; the uncached AMD64 execution preflight passed. Registry manifest/config hashes, OCI source labels, platform and immutable tag resolution were verified for every image. The private registry route retained certificate and hostname verification after public Cloudflare reads returned 403; this did not trigger a rebuild or credential change.

The [native bundle](ASTRA-M04-DIAGNOSTIC-NATIVE-BUNDLE-VERIFIED.json) passes all 13 file checksums, revision checks and Linux AMD64 ELF checks, and contains Codex 0.150.1. Its archive SHA-256 is `78fe85808d951d5e28e3b8680a6cc1a89fcdbfca22d147f577b91b73e0305d79`. The packaging command initially left two untracked output files in `dist`; the clean-source gate correctly stopped assembly. [Only those generated outputs were relocated](ASTRA-M04-DIAGNOSTIC-BUNDLE-OUTPUT-RELOCATION.json), and assembly then passed from a clean source checkout. No SBOM, signature or verified provenance attestation is claimed.

[Pin validation](ASTRA-M04-DIAGNOSTIC-RELEASE-PIN-VALIDATION.json) independently confirms that Helm changes contain only the seven image/revision substitutions and corresponding execution-policy image hashes. The actual Argo Finance overlay renders 53 resources and passes strict server dry-run. The Finance data generation, retention policy, single API writer, existing permissions and budgets are unchanged. Hosted creation, Coding Reliability V2 and Kubernetes native-host execution remain disabled.

## Database compatibility and recovery

The [schema-54 archive](ASTRA-M04-PRE0055-ARCHIVE-VERIFIED.json) is immutable. The [actual new API running on its isolated copy](ASTRA-M04-DIAGNOSTIC-CLONE-MIGRATION-VERIFIED.json) reaches schema 55 with integrity and foreign-key checks passing, preserving every original column in all 81 tables. Historical evaluations receive the explicit full-qualification scope. The source archive remains schema 54.

Once the live database reaches schema 55, source `92f8f1b` is the minimum compatible reader. Do not roll back to source 19 or another schema-54 binary against that database. Prefer a compatible correction; restoring the old archive would lose newer records and requires a separate data-recovery decision. Retain the verified archive and new release artifacts.

## Next gate

Observe the exact Argo pin revision, five deployment image IDs, configured worker/runner identities, API/UI revisions and unchanged Finance generation. Run the prepared read-only live preservation check before any diagnostic writes. Then collect five minutes of internal service health and fresh scoped metrics/logs. Public console access returned HTTP 403 before this pin; internal observation is not proof of public ingress acceptance.

After those checks, run the single `python-contract` Onboarding diagnostic through the real gateway, inspect its first meaningful failure if any, and run the explicit control against its retained public input. Continue the bounded stage canaries only as their boundaries become trustworthy. A diagnostic pass remains distinct from M04E connected-loop proof and M04F full frozen qualification. No Finance production approval is inferred.
