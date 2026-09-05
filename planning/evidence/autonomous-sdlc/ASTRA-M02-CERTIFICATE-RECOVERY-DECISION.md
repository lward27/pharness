# ASTRA M02: Shared certificate-controller recovery decision

Observed 2026-09-05 UTC in `lucas_engineering`.
GitOps baseline: `fa27225c4c33b710ce24708e17fd39ac05ab6aeb`.
Status: owner selected the supported 1.20.3 path; all six sequential upgrade steps
passed. The final accepted GitOps revision is
`7074fdb24cafeee8aaa60ccaef529559b5bb2efc`. This is shared-controller maintenance
inside the authorized cluster; no Finance application image was changed.

## What changed after token rotation

The owner replaced `cert-manager/cloudflare-api-token`, key `api-token`, at
2026-09-05 01:00:34 UTC. All three Finance challenges subsequently became
`presented=true`. Their expected public TXT records were found through both
configured recursive resolvers and both authoritative Cloudflare nameservers.
No token or DNS challenge value is included in this evidence.

At the initial observation, the old July ACME orders then returned `404: No order for ID` and became errored.
Their challenges were expired. Cleanup failed with Cloudflare error 7003 because
cert-manager 1.14.5 constructs `/zones//dns_records/...` without a zone ID.
This matches the upstream [Cloudflare API compatibility bug](https://github.com/cert-manager/cert-manager/issues/7540).
The token was working; repeated token replacement was not the next repair.

A fresh 09:10 UTC check corrected the earlier conclusion that the upgrade was
required to finish renewal: all 31 certificates had renewed on 1.14.5 before the
first upgrade. The three Finance certificates are Ready with December 4 expiry
dates. The earlier cleanup error was real, but it did not remain a renewal blocker.
Do not attribute successful renewal to the subsequent controller upgrade.

## Recorded decision

| Choice | Scope | Consequence |
| --- | --- | --- |
| Supported release, recommended | Sequential GitOps upgrades through 1.15.5, 1.16.5, 1.17.4, 1.18.6, 1.19.6, and 1.20.3 | Reaches a supported branch for Kubernetes 1.34; requires reviewing and controlling broader default changes |
| Minimum immediate compatibility repair | Sequential GitOps upgrades through 1.15.5 and 1.16.5 | Fixes Cloudflare cleanup before the 1.18 default changes, but leaves an unsupported controller branch |

The owner selected the supported release. Both choices used the existing `charts/cert-manager` wrapper and Argo application. No
parallel controller, provider adapter, cluster rebuild, or TLS bypass is proposed.
Version tags were checked against official releases. The vendor recommends
[one minor version at a time](https://cert-manager.io/docs/installation/upgrade/);
[1.20 is currently supported on Kubernetes 1.34](https://cert-manager.io/docs/releases/).

The material downstream changes begin in 1.18: default private-key rotation changes
from Never to Always, and default CertificateRequest retention changes from
unlimited to one. These must not silently remove existing history or change
certificate contracts. Review current issuer/certificate ownership and explicit
settings before crossing that boundary. See the
[1.18 upgrade notes](https://cert-manager.io/docs/releases/upgrading/upgrading-1.17-1.18/).

## Executed sequence and acceptance

| Controller version | Exact merged GitOps revision | Result |
| --- | --- | --- |
| 1.15.5 | `a8cfd08527a17193c286daa91a15102e52955f55` | Accepted |
| 1.16.5 | `545d09c7b9861b432c011e6f848f22c9a02d81f5` | Accepted |
| 1.17.4 | `43f9bd33c8a082b9ac755e447704a42a0e21400a` | Accepted |
| 1.18.6 | `ef96c5d4050a1f763d9444e10260cc77b37a6a0a` | Accepted |
| 1.19.6 | `be91ca643a54953f07050765c0465ad698b2a54c` | Accepted |
| 1.20.3 | `7074fdb24cafeee8aaa60ccaef529559b5bb2efc` | Accepted |

Every step passed Helm lint/render, namespace-correct server dry-run, exact Argo
revision and successful sync, all three controller deployments ready, and admission
startup checks. The final observation is 2026-09-05 09:43:57 UTC. See the individual
`ASTRA-M02-CERT-MANAGER-v*.json` records in this directory.

Before crossing 1.18, authoritative Ingress/Certificate sources explicitly preserved
`rotationPolicy: Never` and `revisionHistoryLimit: 2147483647` (the largest supported
32-bit value, preserving effectively unlimited history). All 31 live Certificates
have those settings, remain Ready, and retain all 78 original CertificateRequest
identities. The PHarness chart change merged as
`97f65557870154c9b02ba5a71d83a006ad6e638e`; its compiled application images stayed
unchanged. The manually synchronized yfinance Application received only its Ingress
annotation update, with no deployment or image change.

No Secret values, CRDs, ACME Orders, Challenges, or finalizers were manually deleted.
No manual renewal was needed. Normal TLS verification passes on all three Finance
public endpoints; Cloudflare returns HTTP 403 to automated probes. Direct origin
probes through the existing ingress, using each real hostname and normal certificate
validation, return HTTP 200. These are separate observations, recorded in
[ASTRA-M02-HTTPS-VERIFICATION.json](ASTRA-M02-HTTPS-VERIFICATION.json).

Keep the previous rendered manifests and metadata evidence for each step. Recovery
must preserve CRDs, certificate material, and history. A rollback that could lose
data or require finalizer removal needs a separate exact decision.
The token fix, a successful render, and healthy controller Pods each prove only
their own boundary; the M02 TLS prerequisite is satisfied, while staging, build, permission, and
rollback-artifact gates remain open.
