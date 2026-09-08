# ASTRA M04: Retained intermediate artifact build

Source `5a409453d28d68d22b12899a312df763c51e48dd` completed one successful Mac/Rancher Desktop build at 2026-09-08T18:36:49Z. All seven Linux AMD64 images were published and independently verified by digest/config/source identity. The native bundle has matching source revision, matching outer SHA-256 and 13 verified internal checksums; no Mac resource metadata is present.

This source was **not deployed**. The [release hold](ASTRA-M04-5A40945-RELEASE-HOLD.json) was recorded after a concrete shared-target protocol policy bug was found. [PR 392](https://github.com/lward27/pharness/pull/392) fixes that gate and merges as `d675159cb205d222616d253d311706be1889bb91`. The combined release must build all artifacts from that later merged source. The pre-existing sourcee508 deployment remains the observed rollback baseline; no Finance production action or new model diagnostic occurred during this intermediate build.

Evidence:

- [Uncached AMD64 preflight](ASTRA-M04-5A40945-AMD64-PREFLIGHT.json), [single build operation](ASTRA-M04-5A40945-BUILD-OPERATION.json), [complete native build receipts](ASTRA-M04-5A40945-RELEASE-BUILD.json), and [independent registry verification](ASTRA-M04-5A40945-ARTIFACTS-VERIFIED.json).
- [Five-minute pre-release service baseline](ASTRA-M04-5A40945-BASELINE-SERVICE-WINDOW-R2.json). It describes the unchanged e508 deployment, not a new rollout.
- [Build phase timing](ASTRA-M04-5A40945-BUILD-TIMING.json): 2,091 seconds total; seven reported layer-push phases sum to 1,192 seconds, about 57 percent. No upload error occurred in this build. This does not resolve the preceding intermittent registry incident or prove a packaging optimization.

The private full build log remains outside Git because registry upload URLs may contain temporary authorization. Public receipts and its byte hash are retained. Artifact verification does not claim SBOM, signature, provenance-attestation, model-qualification or runtime acceptance.
