# ASTRA M02: Finance platform readiness evidence

Status: M02 platform prerequisites accepted on 2026-09-05. Runtime contract
registration and application delivery acceptance remain in their owner milestones.
Observed 2026-09-05 UTC in `lucas_engineering`.
PHarness implementation source is `fd740927110366a983de6bb0d3bc6c576577708b`;
compiled live runtime is still `83a2689c877a3f48688d1d457c34e83474698c46`.
Verified GitOps configuration is `137262f4377c5f1d19379f73e83249d66d09a0fd`.
GitOps evidence-only PR 51 is merged at `19e68c4be4adb35c79bb5c11efb95410c5e71f21`.
This record supersedes the earlier missing-artifact and credential blockers while
retaining their original observations in the linked JSON evidence.

## Verified platform result

Both Finance applications have internal, isolated staging deployments under Argo.
Both production and staging use immutable image references. Finance certificates
are valid, the approved cert-manager upgrade is complete, the replacement Mac
build service can publish large AMD64 images, and the separate worker identities
passed their bounded repository checks. These establish platform prerequisites;
no autonomous application request has completed this program's lifecycle yet.

| Boundary | Observed result | Evidence |
| --- | --- | --- |
| Finance HTTPS | Normal chain/hostname verification passes; origin health returns 200. Cloudflare returns 403 to automated public HTTP probes, recorded separately. | [HTTPS](ASTRA-M02-HTTPS-VERIFICATION.json) |
| Certificate controller | Six sequential upgrades reached 1.20.3; all 31 Certificates Ready and all 78 retained CertificateRequest UIDs preserved. | [Decision and per-version evidence](ASTRA-M02-CERTIFICATE-RECOVERY-DECISION.md) |
| Frontend baseline | Production and staging pin the original running `248437be...` digest. Original build-source provenance remains unknown. | [Current identities](ASTRA-M02-PLATFORM-VERIFIED-IDENTITIES.json) |
| Backend baseline | The original `f1cfc06f...` descriptor graph was restored byte-for-byte from retained node content; staging pulled it and became Ready. Production was not restarted. | [Artifact restoration](ASTRA-M02-YFINANCE-ARTIFACT-RESTORATION.json) |
| Staging isolation | Ten tested production connections denied; three necessary connections allowed. Backend Yahoo history still returns 200. | [13 connection checks](ASTRA-M02-STAGING-ISOLATION-AFTER.json), [upstream HTTP](ASTRA-M02-STAGING-UPSTREAM-AFTER.json) |
| Mac BuildKit | Existing Tekton Service and mTLS identity reach the M1 Mac. Uncached AMD64 execution and a 112 MiB random-layer private TLS push passed; exact digest pulled and ran with network disabled. | [Large build/push/pull/run](ASTRA-M02-MAC-PRIVATE-LARGE-UPLOAD.json) |
| Registry continuity | Upload-signing key and Pod identity persist across Argo renders/revisions after the narrow GitOps correction. | [Stability](ASTRA-M02-REGISTRY-STABILITY.json) |
| Worker identities | Application writers/observers and GitOps observer passed; owner rotated the invalid GitOps writer, whose isolated push check then passed. | [Initial checks](ASTRA-M02-WORKER-CAPABILITY-PREFLIGHT.json), [writer recovery](ASTRA-M02-GITOPS-WRITER-ROTATION.json) |
| App telemetry | Fresh staging yfinance request metrics, exact-Pod application logs, and environment-scoped traces; fresh frontend Nginx logs. | [Scoped signals](ASTRA-M02-STAGING-TELEMETRY-VERIFIED.json) |

## Changes and why they were necessary

GitOps PR 39, merge `e036269a173a9e2563bb6011854b561007b49828`, pinned the
frontend's existing image, mounted non-secret runtime configuration, and created
`yfinance-staging` and `finance-frontend-staging` in `apps-staging`. Neither has a
public Ingress or mounted service-account token. The current frontend still does
not consume `/runtime-config.json`; that source change remains M11.

The first staging backend could not pull production's digest. Its exact retained
manifest, configuration and layers were recovered through a bounded read-only
node-cache export, content-hashed, and restored to the registry. This was not a
rebuild: source `da4e3cd8a4c33d2b359e4e521525203da32ecf18` remains the historical
PipelineRun's recorded source for digest
`sha256:f1cfc06fcac62d7c37a4d7dc87237e2abe02df0d9c3824a7521c5359058879c1`.
The same source's surviving mutable tag pointed elsewhere and was not substituted.
Staging became Ready at 10:11:29 UTC. Its exact imageID and unchanged production
Pod UID are recorded in the restoration and current-identity evidence.

The first network probes found that backend public HTTPS egress also reached the
three Finance production hostnames through Cloudflare. PR 49, merge
`516c7fcbda998b337303fc761f6e1a9efcc4497f`, excluded all 15 published Cloudflare
IPv4 ranges in addition to private ranges. All three production names currently
resolve there; there is no IPv6 egress allowance. The second set of application-
labelled init-container probes confirmed frontend/backend production isolation,
while staging backend, Yahoo and the telemetry collector remained reachable.
These probe Pods never became Ready service endpoints and sent no production
mutation requests. Recheck the boundary if ingress/DNS or upstream providers change.
It also blocks other Cloudflare-hosted upstreams; it is a finite network boundary,
not a general hostname firewall or protection against arbitrary external relays.

The frontend's same-origin browser policy blocks compiled production service URLs.
Nginx permits GET/HEAD only to staging yfinance, returns 403 for proxy writes and
503 for unbound services, and serves runtime configuration with `no-store`.
Document, configuration, health and history probes passed. This prepares isolation
and configuration delivery; it does not claim the old application can use it yet.

Certificate renewal happened on the old controller after token rotation. The
transient Cloudflare cleanup error was not a continuing renewal blocker. The
owner-approved maintenance upgrade then reached 1.20.3 at
`7074fdb24cafeee8aaa60ccaef529559b5bb2efc`, preserving private-key rotation and
history defaults explicitly. No Secret, CertificateRequest, ACME object, CRD,
finalizer or Finance data was deleted.

The owner selected the M1 Mac because the desktop builder is powered off. PR 48,
merge `e70ac07b28904777a087dee6daabbc213faa8d51`, connected the existing Tekton
BuildKit Service to the Mac VPN address. Small-image testing exposed registry
upload-key rotation from the Helm chart; PR 47, merge
`cf95a7500305c5c5e2c9a9eebe40be6ff38d3cd8`, preserves only the two generated fields.
Real PHarness runner publication then hit Cloudflare's large-upload limit.
PR 50, merge `137262f4377c5f1d19379f73e83249d66d09a0fd`, allowed the Mac's exact
VPN address through the existing private registry gateway. Routing changes are
confined to the dedicated BuildKit container. TLS hostname and private-CA
verification remain enabled. The large probe completed in 212 seconds within its
unchanged 360-second bound. The Mac, VPN, Rancher Desktop and forward must stay up;
M12 must prove the accepted operating arrangement unattended.

## Validation and limits

Helm/Kustomize render, lint and applicable server dry-runs passed before GitOps
merges. Argo's exact synchronized revision, live imageIDs, retained registry Pod,
normal TLS, HTTP behavior and network enforcement were checked independently.
Original failed probes and initial unscoped/unauthenticated telemetry queries are
retained alongside their corrected results; they are not counted as passing checks.

The authenticated Finance generation is `dbgen_finance_20260827`. Application
writer dry-runs establish repository push permission; observer checks establish
read access to PRs, rules, checks and statuses. The owner supplied a fine-grained
GitOps token with Contents and Pull Requests read/write for `lucas_engineering`.
No token values were printed or written to evidence. These dated checks expire;
M07/M09 must recheck actual permissions and exact-head behavior during execution.
They do not prove that a source or production GitOps merge has occurred autonomously.

Mimir must be queried through its configured gateway tenant. Match
`job="apps-staging/yfinance-wrapper"` and record sample freshness. Loki queries
include namespace, container and exact Pod to exclude diagnostic probe logs.
Tempo uses both service name and `resource.service.namespace="apps-staging"`.
No frontend application traces or request-metric SLO have been established. Fresh
Nginx logs are available; M08 owns the bounded frontend verification contract.
This telemetry capture is prerequisite discovery, not a staging release verdict.

The live API currently lists no PipelineContracts or DeploymentContracts. The
[finite coordinates](ASTRA-M02-FINANCE-DELIVERY-BINDINGS.md) identify their required
resources; M05 registers validated contract IDs before enabling hosted creation.
The existing protected production contract API admits only yfinance; M05 is adding
finite frontend declaration while preserving the legacy execution restriction.
M07 owns the actual frontend pipeline. Missing runtime declarations must never be
replaced by invented IDs or synthetic successful build results.

## Remaining program gates and recovery

M04's previous evaluation was evicted after fixture scratch data accumulated in
its fixed temporary volume. The repair is merged at PHarness `fd740927...`;
the seven-image/native-bundle release is being assembled. Fresh protocol and two
frozen qualifying runs are still required. M05 is unpublished code preparation;
M06–M12 remain unaccepted. No application patch, production approval, automatic
rollback, runtime-verification window or unattended day is claimed here.

Recover only through the recorded GitOps paths. Keep production digest baselines,
Finance generation and audit history. Repair staging in its overlay; never redirect
it to production. Return BuildKit to the desktop only after it is available and
qualified, with no active build using the Mac. Do not remove namespaces, PVCs,
retained history or image caches as part of this recovery procedure.
