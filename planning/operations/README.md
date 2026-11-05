# Current operational playbooks

Only currently useful, bounded validation procedures belong here. Every command
must be rechecked against the current revision, context, namespace, identities,
and effect boundary before it runs.

## Playbooks

- [Conference demo](runbooks/conference-demo.md) rehearses typed allow/deny decisions and browser-visible audit evidence.

- [Campus OIDC sessions and course authorization](campus-oidc-auth.md) configures the disabled-by-default `/api/campus/*` boundary, its cookies, CSRF, logout and IdP swap.

- [Campus lab session journal](campus-session-journal.md) describes the lab-session states, fencing, crash recovery, provider contract and the restore procedure for migration `0058`.

- [Campus lab admission and session API](campus-lab-admission.md) configures the static lab catalog and slot count, lists the launch, status, stop and reset routes with their error codes, explains transactional quota admission and resets, and the restore procedure for migration `0059`.

- [Campus image, dependency and installer gates](campus-image-approval.md) approves lab template image digests through SBOM, vulnerability and signing evidence, enforces the digest allowlist at admission, and pins package indexes and per-session caches.

- [Campus broker tokens and the sandbox credential boundary](campus-broker-tokens.md) describes the scoped broker tokens, their TTL, rotation and per-request revocation, the broker listener and fake inference route, the sandbox Pod-spec contract, the provider integration contract and the restore procedure for migration `0060`.

- [Campus session Jobs in lab slots](campus-session-jobs.md) describes the Kubernetes provider, the slot guardrails, session limits, the cluster qualification script and residual risks.

- [Exact-policy gateway preflight](ASTRA-M04-GATEWAY-PREFLIGHT.md) binds protocol evidence to the selected stage policy before diagnostics or qualification.

- [Minisforum Windows local-model setup](ASTRA-MINISFORUM-WINDOWS-LOCAL-MODELS.md)
  prepares LM Studio for the existing PHarness gateway and separates initial setup from qualification.
- [`release-verification-smoke-playbook.md`](runbooks/release-verification-smoke-playbook.md)
  exercises read-only release verification and durable completion evidence.
- [`sqlite-backup-and-restore.md`](runbooks/sqlite-backup-and-restore.md)
  covers the scheduled `pharness-admin` archives, their verification, and
  restoring into a new claim. Rehearsal records live under
  [`rehearsals/`](rehearsals/).
- [`tekton-e2e-smoke.md`](runbooks/tekton-e2e-smoke.md) describes the bounded
  inert Tekton fixture.
- [`tekton-executor-smoke-playbook.md`](runbooks/tekton-executor-smoke-playbook.md)
  describes the reviewed console and CLI flow for that fixture.

Older manual API/CLI smoke sequences were superseded by the environment-ready
WorkItem flow and operator cockpit. They remain available under
[`../archive/runbooks/`](../archive/runbooks/) but are not current run
instructions.

Before an effectful smoke, state the exact cluster context, namespace,
application/repository target, expected mutation, success criteria, and cleanup
ownership. Never copy credentials into a playbook or command transcript.

## Starting a release observation window

After exact Argo and running-image verification, confirm that the expected
availability metrics are positive and fresh, and inspect their underlying sample
timestamps. Samples used to start the window must follow the verified rollout;
a range-query timestamp alone can still select an older pre-readiness sample.
Then start the complete stated observation period. Retain failed or interrupted
windows separately; never trim the first point, reuse a partial window as complete,
or substitute direct health checks for telemetry. The [sourcee508 observation
record](../evidence/autonomous-sdlc/ASTRA-M04-RELEASE-WINDOW-START.md) documents the
observed timing problem and the added start check.
