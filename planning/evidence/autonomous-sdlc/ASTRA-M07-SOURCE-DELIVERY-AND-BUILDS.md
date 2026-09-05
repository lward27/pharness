# ASTRA M07: Finance delivery preparation and real builds

Status: independent pipeline and packaging prerequisites implemented; source
controller integration, qualified coding, build acceptance, and M07 acceptance open.
Observed 2026-09-05. This work does not bypass M04/M06 or count as either M11
autonomous application change.

## Source and deployment identities

| Boundary | Exact revision | Result |
| --- | --- | --- |
| GitOps finite pipelines, PR 52 | `bd36ae698951ab36a4e7362562eba34a997a70c8` | Merged; Argo Synced/Healthy; application Deployments unchanged |
| GitOps restricted identity, PR 53 | `491f081e3ea6e639528a98cce43466cc7858fdcc` | Merged; Argo Synced/Healthy at 15:09 UTC |
| yfinance packaging, PR 6 | `efa6294954b01a089a65419c85542b8fc2f95c83` | Merged; Tekton and independent registry verification passed |
| Frontend packaging, PR 7 | `c4d64f9242f2955064f99e659bf1648ce6bc4273` | Merged; sequential Tekton and registry verification passed |

The PRs are [pipelines](https://github.com/lward27/lucas_engineering/pull/52),
[build identity](https://github.com/lward27/lucas_engineering/pull/53),
[backend packaging](https://github.com/lward27/yfinance_wrapper/pull/6), and
[frontend packaging](https://github.com/lward27/finance-frontend/pull/7).
[Application merge record](ASTRA-M07-PACKAGING-MERGES.json).

## Implemented boundaries

Both finite Finance Pipelines require a full lowercase commit, compare it with
the actual checkout, pass it into the root Dockerfile, and return `SOURCE_COMMIT`,
`IMAGE_URL`, and `IMAGE_DIGEST`. They use the existing remote BuildKit Task on the
owner-authorized Mac. The result parser rejects malformed or missing digests.
TLS verification stays enabled. Neither Pipeline contains a deployment task.

The obsolete frontend webhook previously built a mutable image and restarted
production. It is now disabled in GitOps. Argo removed that application's trigger
resources; other applications' hooks remain unchanged. The
[deployment proof](ASTRA-M07-PIPELINE-DEPLOYMENT.json) compares both Finance
production Deployment specifications, generations, Pod UIDs, and image identities
with the [baseline](ASTRA-M07-PRODUCTION-BEFORE-PIPELINES.json).

`pharness-finance-build` has no RBAC binding and disables Kubernetes token mounts.
Live checks denied production Deployment patches, PHarness Job creation, and
Tekton Secret reads. Clone tracing is disabled and TLS verification required.
Other applications retain their existing default identities. Finance runs must
explicitly select the restricted account. PHarness dispatch enforcement is tested
in [PR 336](https://github.com/lward27/pharness/pull/336), held for the current
immutable release to finish. [Identity evidence](ASTRA-M07-BUILD-IDENTITY-VERIFIED.json).

The rendered guards passed 36 checks, covering invalid revisions, checkout
mismatches, malformed digests, finite repository/image bindings, retired triggers,
and identity constraints. Helm lint and Kubernetes server dry runs passed. The
checker and output are committed in GitOps `ops/finance-delivery/`; no additional
test framework was introduced.

## Packaging and tests

yfinance installs hashed, pinned binary dependencies from its committed lock and
checks consistency, using the same pinned Python 3.11 base as PHarness's runner.
Automatic instrumentation packages were made explicit after inventorying the
running baseline. All 51 previous locked versions were preserved, with 17
explicit/transitive instrumentation packages added. Unbounded dependency
installation and build-time bootstrap resolution are removed.

The frontend uses the same pinned Node 24 base as its runner and a pinned Nginx
runtime. Its npm lock is unchanged. Both Dockerfiles execute the existing tests
and retain source/revision OCI labels. Application source and tests are unchanged;
the READMEs now describe the actual commands. Nonblocking upstream calls and
runtime configuration loading remain the later M11 requests.

Clean local Linux AMD64 builds passed 37 backend tests and 49 frontend tests.
Frontend lint had zero errors and one existing hook-dependency warning at
`src/pages/Dashboard.jsx:140`. Isolated backend health/invalid-input and frontend
Nginx/SPA/asset smoke checks passed. External calls and telemetry exports were
disabled for these packaging checks; they are not staging or telemetry acceptance.
[Packaging validation](ASTRA-M07-PACKAGING-VALIDATION.json) records exact branch
revisions, images, source labels, locks, log hashes, and limitations.

## Real builds and remaining gates

The backend [PipelineRun](ASTRA-M07-YFINANCE-PIPELINERUN.json) and sequential
[frontend PipelineRun](ASTRA-M07-FRONTEND-PIPELINERUN.json) both succeeded. Each
used the actual merged commit, restricted account, Linux AMD64 placement, a
dedicated 1Gi workspace, and the existing 60-minute timeout. All four Tasks in
each run passed. Their Pods had no Kubernetes API token mount.

Independent verified-TLS registry reads confirmed the tag, manifest content hash,
configuration content hash, platform, source repository, and actual merged SHA:

| Application | Registry digest | Complete evidence |
| --- | --- | --- |
| yfinance | `sha256:33f1a08b74c82fb5dc01ef0ebef8a1fa5e2fc0ac78be17dadd1f74bbf1e319ca` | [Backend build](ASTRA-M07-YFINANCE-BUILD-VERIFIED.json) |
| Frontend | `sha256:387a1f829181dc283dc2033ae204a754f863616ad86937a749d07ff8549df987` | [Frontend build](ASTRA-M07-FRONTEND-BUILD-VERIFIED.json) |

The records retain TaskRun identities, executed spec hashes, step image identities,
results and sanitized build logs. They do not claim an SBOM or signature. These
are program-operated real builds, not WorkItem-driven autonomous results, and
neither run updates an application Deployment.

Both Finance branches currently lack required checks/protection. Two proposed
source-check PRs passed on GitHub using these release Dockerfiles. The exact
[protection decision](ASTRA-M07-SOURCE-MERGE-DECISION.md) is pending owner input
because requiring PRs on both main branches changes the developer workflow.

M07 still requires automatic source publication, enforced current required checks,
exact verified-source merge, interruption recovery, build dispatch, and independent
registry source/digest verification. Stale heads, changed base trees, conflicts,
failed builds, and missing or mismatched results must stop delivery. A green Task
or an OCI label alone does not prove that chain.

M04 remains failed and hosted creation disabled. Production is unchanged. M06 owns
the PHarness database rollback floor; these pipeline/packaging changes do not migrate
application data. Retain failed build history and stop before deployment. Restoring
the retired mutable frontend production webhook is not a recovery procedure.

## Durable source progression follow-up

[PR 338](https://github.com/lward27/pharness/pull/338), source `abca81f1f95cac45cd09e0143ae55af678fe3b7e`, adds automatic source publication and bounded observation under the recorded workflow authority. The [implementation evidence](ASTRA-M07-SOURCE-PROGRESSION.md) records 258 distinct API tests across the full and final targeted runs. It is not in the sealed 48c77b7 release. Automatic GitHub merge, source-to-build progression, missing-callback reconstruction and live acceptance remain open.

PR 336 merged at 16:21 UTC as `cfae10c29afd73d0e8a4239e8c459d3236e358a7`; PR 338 merged at 16:22 UTC as `f1239eab36c2078f463aab766f5946546a51f231`. They are source changes awaiting a subsequent complete release; the running image revision remains 48c77b7.
