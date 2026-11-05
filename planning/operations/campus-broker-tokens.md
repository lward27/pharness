# Campus broker tokens and the sandbox credential boundary

Status: the token model, the journal-backed issuer, the broker listener with
a fake inference endpoint, and the sandbox Pod-spec contract are in place.
The Kubernetes provider (LUC-125) follows the
[provider integration contract](#provider-integration-contract) (LUC-130).
It was verified live in a lab slot under gVisor
([Verification status](#verification-status)).
Migration `0060` adds the token table (see
[Restore procedure](#restore-procedure)).

| Piece | Location |
| --- | --- |
| Token model, hashing, per-request decision | `crates/pharness-core/src/campus_broker_token.rs` |
| Sandbox Pod-spec contract and broker binding | `crates/pharness-core/src/campus_sandbox_pod.rs` |
| Journal-backed issuer | `crates/pharness-store/src/campus_broker.rs` |
| Issue, authenticate, list, revoke SQL | `crates/pharness-store/src/sqlite/campus_broker.rs` |
| Schema | `crates/pharness-store/migrations/0060_campus_broker_tokens.sql` |
| Broker listener and fake inference | `crates/pharness-api/src/app/campus/broker.rs` |
| Refusal on worker, operator and agent-host routes | `crates/pharness-api/src/app/auth.rs` |

## What a sandbox holds

Everything inside a lab Pod is student-controlled: code, shell, terminal and
every process. The Pod therefore holds one credential only, the session's
broker token, and nothing a student could use against the platform:

- no worker token, operator token, model-provider key, Git or GitOps
  credential, and no Kubernetes service-account token
  (`automountServiceAccountToken: false`, no projected volumes, nothing
  mounted under `/var/run/secrets` or `/run/secrets`);
- no host namespaces and no `shareProcessNamespace`, so `ps` shows only the
  student's own processes;
- exactly one container. A trusted broker is never a sidecar: a sidecar
  shares the Pod's network namespace and volumes. The broker runs in the
  control plane and is reached over the network;
- only literal environment values, none credential-shaped, no `envFrom` and
  no `valueFrom`. The only platform variable is `CAMPUS_BROKER_URL`, which
  names the broker and carries no secret;
- the token as a file at `/var/run/campus-broker/token`, from a Secret
  volume mounted read-only without `subPath`, so the kubelet replaces it in
  place on rotation. Clients must read the file on every request.

`check_campus_sandbox_pod_spec` enforces all of this on the exact `PodSpec`
a provider is about to submit. The SDLC worker Job manifests fail it (they
carry `PHARNESS_WORKER_TOKEN` through `valueFrom` and a service account); a
test in `crates/pharness-api/src/dispatch.rs` keeps it that way.

## Token model

| Property | Value |
| --- | --- |
| Format | `pcbt1_` followed by 43 base64url characters (256 random bits from the system CSPRNG) |
| Storage | `campus_broker_tokens.token_hash` only: SHA-256 over `pharness-campus-broker-token-v1\0` and the token, hex. The raw token exists in the issuer's return value and in the session's Secret, nowhere else |
| Audiences | `inference` (the only audience a sandbox may hold) and `tools` (reserved for trusted tool brokers). A token names exactly one |
| Binding | one lab session, its slot occupancy (`slot_id`, `slot_generation`) and its lab generation |
| Lifetime | at most 900 s (`CAMPUS_BROKER_TOKEN_MAX_TTL_SECONDS`, also a table `CHECK`), never past the session's `expires_at` |
| Rotation | providers re-issue after 450 s (`rotate_at`, also written to the Secret annotation `pharness.dev/campus-broker-token-rotate-at`). At most two live tokens per session and audience; issuing a third revokes the oldest as `superseded` |
| Issuance | only while the session is `provisioning` or `ready`, desired `ready`, not revoked, unexpired, on the named occupancy, and its lab's current generation. Issuance and stop requests take the same write lock, so no token is issued after a stop |
| Immutability | triggers refuse deletes and every update except one revocation |

### Per-request check and revocation timing

Every broker request reads the token, its session, the lab and the owner in
one query and decides with `CampusBrokerTokenFacts::evaluate`. Nothing is
cached. A request is refused, with a `code`, when:

| Code | Meaning |
| --- | --- |
| `malformed` | no single `Authorization: Bearer pcbt1_...` header (cookies, query strings, worker or operator tokens are never read) |
| `unknown_token` | well formed but not issued |
| `wrong_audience` | issued for another service |
| `token_revoked` | revoked by a stop, reset, failure, supersession or an operator |
| `token_expired` | past its own `expires_at` |
| `session_inactive` | the session is not `ready` (a token issued while provisioning works only once the session is ready), was asked to stop, or was revoked |
| `session_expired` | past the session's `expires_at` |
| `generation_mismatch` | the session no longer holds the occupancy, or is no longer its lab's current generation |

The trigger `campus_broker_tokens_revoke_with_session` revokes a session's
live tokens in the same transaction that asks it to stop, revokes it, or
moves it out of `provisioning`/`ready`. A stop or reset therefore ends the
token on its next request, before any reconciler runs and well inside the
60-second target. The 15-minute TTL only bounds how long a copied token
would stay useful if every other check were bypassed.

Operators can revoke a session's tokens with
`SqliteStore::revoke_campus_broker_tokens(session_id, reason, now)`; there
is no HTTP route for it yet.

## Broker listener

| Variable | Meaning |
| --- | --- |
| `PHARNESS_CAMPUS_BROKER_BIND` | Address of the broker listener, for example `0.0.0.0:4790`. Unset disables it. Requires `PHARNESS_CAMPUS_ENABLED=true` and a port different from `PHARNESS_BIND` and the metrics listener, so a sandbox network policy can admit the broker port alone |

The listener serves only these routes. Nothing else answers on it (`404`),
and none of them answers on the main listener.

| Method and path | Audience | Success |
| --- | --- | --- |
| `GET /v1/token` | `inference` | `200 { token_id, audience, session_id, lab_generation, expires_at }` |
| `POST /v1/inference/chat/completions` | `inference` | `200`, an OpenAI-shaped `chat.completion` from the fake model `campus-fake`, plus `campus_session_id` |

The fake inference endpoint calls no model provider. It accepts 1-64
messages with roles `system`, `user` or `assistant`, an optional `model`
that must be `campus-fake`, and bodies up to 64 KiB. The session it answers
for always comes from the token, never from the body. Refusals are
`401 { error, code }` with the codes above; a broken journal answers `503`.
Responses carry `Cache-Control: no-store`, and logs record the token id and
session, never the token.

Worker (`/api/internal/*`), agent-host and operator middleware refuse any
`Authorization` header whose bearer value starts with `pcbt1_` with
`401 campus broker tokens are not accepted on this route`, including in
local mode with no operator tokens configured. Campus browser routes read
only the session cookie.

## Provider integration contract

For LUC-125 (`CampusExecutionProvider` on Kubernetes). The provider runs in
the control plane; only the objects it creates reach the sandbox.

1. Obtain the issuer from the Campus runtime when constructing the provider:
   `CampusRuntime::broker_token_issuer(&store, "<provider name>")` returns
   `Arc<dyn CampusBrokerTokenIssuer>`.
2. In `provision(workload, ...)`, before submitting the Pod:
   1. `let binding = CampusSandboxBrokerBinding::new(&broker_url, &workload)?`,
      where `broker_url` is the in-cluster URL of the broker listener (a
      Service name; loopback, link-local, metadata addresses, credentials and
      query strings are refused).
   2. `let token = issuer.issue(&workload, CAMPUS_SANDBOX_AUDIENCE).await?`.
      `NotIssuable` means the session was stopped or superseded: do not
      provision. `Unavailable` is retryable.
   3. `binding.secret_manifest(namespace, labels, &token)?` and create or
      replace it through the Kubernetes API with the manifest on stdin or in
      the request body, never on a command line or in a log.
   4. Build the `PodSpec` with `binding.env()` in the container `env`,
      `binding.volume()` in `volumes` and `binding.volume_mount()` in the
      container's `volumeMounts`, `automountServiceAccountToken: false` and
      `allowPrivilegeEscalation: false`.
   5. Call `check_campus_sandbox_pod_spec(&pod_spec, &binding)` on the exact
      spec (for a Job, `spec.template.spec`) and refuse to submit on any
      violation.
3. On each observation, read the Secret's
   `pharness.dev/campus-broker-token-rotate-at` annotation; at or after it,
   issue a new token and replace the Secret. The previous token stays valid
   until it expires or a third is issued, so the kubelet's propagation delay
   does not cause failures.
4. Delete the Secret with the workload. Its name,
   `campus_broker_token_secret_name(workload)`, is derived from the fenced
   occupancy, so a later occupancy of the slot never reuses it. Deleting it
   is cleanup only; revocation already happened in the journal.
5. Give the sandbox namespace a NetworkPolicy whose only control-plane
   egress is the broker Service port, and give the Pod a service account
   with no RBAC bindings.

## Restore procedure

Migration `0060` creates one table, one index and three triggers, one of
them on `campus_sessions_journal`; that trigger only writes to the new
table. The migration tests open a schema-59 database with an admitted
session, apply `0060`, and check that every existing row and schema object
is unchanged and that existing sessions still stop; an interrupted `0060`
rolls back and can be retried. The previous runtime refuses a migrated
database with `VersionMissing(60)`, so rollback is a restore of the
pre-release archive as described in
[`campus-session-journal.md`](campus-session-journal.md#restore-procedure).
Tokens issued after the archive are lost, which revokes them; providers
issue new ones on their next observation.

## Verification status

Proven by tests now (`cargo test --workspace`):

- token format, hashing, TTL caps, audience and generation binding,
  rotation and supersession, immutability, and revocation with stop, reset,
  failure, expiry and operator action (store and HTTP tests);
- a broker token is refused by every operator, worker and agent-host route
  in the route inventory and by every Campus browser route, with and without
  operator tokens configured;
- worker and operator tokens, Campus cookies, tokens in cookies or query
  strings, altered tokens and duplicate headers are refused by the broker;
- forged lifecycle reports through every internal route, and forged fields
  in broker requests, write nothing and leave the lab session unchanged;
  lifecycle facts come only from the reconciler's provider observations;
- the Pod-spec contract refuses service-account tokens, host namespaces,
  shared process namespaces, sidecars, `envFrom`/`valueFrom`,
  credential-shaped values, reserved mount paths, writable or `subPath`
  token mounts, and the SDLC worker Job specs.

Verified live on 2026-11-05 (`scripts/campus-egress-cluster-test.sh`, phase
11, and the ignored test `campus_broker_live`). The real Kubernetes provider
used the journal's issuer and launched a session through the controller
identity under gVisor:

- the mounted token authenticates its own session at the broker;
- a scrape from inside the Pod found no broker token and no
  credential-shaped variable in the environment, `/proc/*/environ`, process
  arguments or file names (21 variables, 18 processes);
- there is no service-account mount, and `/var/run/secrets` and
  `/run/secrets` do not exist;
- the token mount is read-only, and `token` is its only file (the egress CA
  is added only for egress sessions);
- rotation: a provider past `rotate_at` replaced the Secret, and the kubelet
  changed the mounted file in place after 65 s. Both tokens stayed valid
  during the overlap;
- stop: the broker refused both the current and the previous token with
  `token_revoked` on the first request after the stop (0 ms), and a later
  rotation was refused;
- delete removed the Job, the Pod and the token Secret in 3 s;
- the slot NetworkPolicies admit the broker port and nothing else of the
  control plane. The Kubernetes API, the kubelet, cloud metadata, the API's
  operator port and DNS are unreachable
  ([campus-egress.md](campus-egress.md#results-2026-11-05-run-035824)).

Not yet verified: broker HTTP from inside the Pod to the production API
(`10.43.130.31:4790`). The PHarness chart does not yet create the broker
Service or enable `PHARNESS_CAMPUS_BROKER_BIND`, and the browser terminal
does not exist yet.

## Residual risks

- The broker listener runs in the API process, which holds every platform
  credential. Its attack surface is two routes behind token checks, but a
  memory-safety or parser bug in that process would cross the boundary.
  Moving the broker to its own Deployment is a later option.
- The Pod-spec check covers the spec the provider submits. Admission policy
  (for example Kyverno or ValidatingAdmissionPolicy) must enforce the same
  rules for anything else that could create Pods in the sandbox namespace.
- Inference is a fake. Real model routing must keep the session from the
  token and add per-session budgets.
- Operator revocation has no HTTP route or audit event yet.
- Tokens are kept forever; retention needs a migration.
