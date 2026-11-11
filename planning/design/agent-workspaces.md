# PHarness agent workspaces: product direction

Status: approved direction, 2026-11-11. Owner decisions recorded below.
Supersedes the hosted SDLC promise in
[product vision and boundaries](product-vision-and-boundaries.md) as the active
product. The [ASTRA program](../programs/autonomous-sdlc/ASTRA-00-PROGRAM.md) is
frozen.

This document states intent. "Today" paragraphs describe code on main and name
the file that proves it; everything else is planned and is not implemented.

## Product promise

PHarness runs agent workspaces that an organization can constrain, govern and
observe. A user launches a workspace from an approved template, works in it
with the agent they choose, on the model provider they or their organization
supply, and the organization decides in advance what that workspace can reach,
spend and run for. When the session ends, its access is revoked and its
resources are confirmed gone.

Campus (course labs for students) is the first deployment. It becomes one
configured preset of the generic product, not the product itself.

## Owner decisions (2026-11-11)

| Decision | Choice | Consequence |
| --- | --- | --- |
| SDLC code | Frozen | No new SDLC features, qualification runs or ASTRA milestones. Security fixes and changes needed to keep the build green are allowed. Nothing is deleted. |
| Delivery model | Self-hosted install per customer | One organization per install. No cross-tenant isolation inside one control plane. Install, upgrade and backup must work without the `lucas_engineering` GitOps repository. |
| Governance depth | Boundary-level, for now | The platform controls the sandbox, the network and inference. It does not see or approve an outside agent's individual tool calls. |

## Who uses it

| Role | Does | Campus name today |
| --- | --- | --- |
| Platform operator | Installs PHarness, sets hard ceilings, approves images | platform |
| Organization admin | Sets organization policy, provider credentials, membership | institution, tenant admin |
| Template owner | Publishes a workspace template for a group | instructor (assignment) |
| User | Launches a workspace, picks agent and model within the template | student |

## Model

- **Organization**: the customer. One per install.
- **Group**: people who share templates and a quota (a course, a team).
- **Workspace template**: a profile (image, resources, network, lifetime,
  budgets, allowed models) plus the policy layers that apply to it.
- **Workspace session**: one running sandbox for one user, launched from a
  template, with one effective policy fixed at launch.
- **Agent**: software that runs inside the session and does work for the user.
- **Provider**: a model API the session may call through the inference broker.

Policy is composed in a fixed order: platform, organization, template, user.
Each layer can only narrow the one above. A layer that asks for more is refused,
not clamped.

Today: `crates/pharness-workspaces/src/campus_policy.rs` implements this
composition with the authorities named platform, institution, assignment and
student. `campus_profile.rs` is the profile envelope.

## The three properties

### Constrained

A session can reach only what its effective policy names.

Today (Campus PoC, see the [operations runbooks](../operations/README.md)):

- gVisor sandbox Pod, one container, no service-account token, read-only root,
  literal environment only (`campus_sandbox_pod.rs`).
- Admission policies and a signed, digest-pinned image allowlist
  (`deploy/campus/`).
- Default-deny network. Approved egress goes through a proxy that allows listed
  hosts on port 443 and checks each request (`campus_egress.rs`).
- Absolute expiry, revoke-first teardown, orphan janitor and slot quarantine
  (`campus_lab_session.rs`, `pharness-store` teardown).

Planned: persistent workspace volumes as a template option; dynamic slot
provisioning in place of five pre-created namespaces; egress routes as install
configuration in place of a compiled list.

### Governable

An organization decides what templates exist and what each may do, and can
revoke a session at any time.

Today: templates and policy layers come from one JSON catalog read at startup
(`pharness-api/src/app/campus/lab_catalog.rs`). Broker tokens are checked
against the session journal on every request, so stop, reset and expiry take
effect on the next request (`campus_broker_token.rs`).

Planned: templates, policy layers and membership in the database with an admin
API and page; per-group and per-user spend limits enforced by the inference
broker.

### Observable

An admin can see what every session is doing and did.

Today: journaled lifecycle transitions and audit events per session, and
Prometheus gauges for sessions, slots and the janitor
(`pharness-api/src/app/metrics.rs`).

Planned: a per-session record of inference requests (model, tokens, cost,
refusals), egress decisions and terminal connections; an admin view of live
sessions; export to the customer's own log and metrics stack.

## Bring your own provider

A session never holds a provider key. It calls the inference broker with its
session token; the broker checks the session and its policy, attaches the
credential and forwards the request.

Today: the broker's inference route is a deterministic fake
(`pharness-api/src/app/campus/broker.rs`). The separate model gateway
(`crates/pharness-model-gateway`) already routes to Fireworks, OpenRouter,
LM Studio, llama.cpp and OpenAI-compatible backends, from a static registry
file and operator-held credential files.

Planned:

1. The broker forwards to the gateway. Model aliases in the template map to
   gateway targets.
2. Provider credentials at three scopes: install, organization, user. A user
   key is usable only in that user's sessions and only if the template allows
   user-supplied providers.
3. PHarness meters tokens and cost itself from provider responses. The LiteLLM
   spike (`deploy/spikes/campus-litellm`) showed gateway counters can
   undercount; they are not the spend authority.
4. An Anthropic-format route next to the OpenAI-compatible one, so agents that
   speak either protocol work unmodified.

## Bring your own agent

An agent is a container image that follows the agent image contract. The
platform approves the image by digest; it does not need to understand the
agent.

Proposed contract, version 1 (not implemented; the variable names are today's
Campus names and will be given neutral aliases):

| Item | Value |
| --- | --- |
| Workspace directory | `/workspace`, writable, owned by the session user |
| Broker address | `CAMPUS_BROKER_URL` |
| Session token | file under `/var/run/campus-broker`, read on every request, rotated in place |
| Model endpoint | `<broker>/v1/inference/chat/completions` (OpenAI-compatible) |
| Egress | `HTTPS_PROXY` and CA variables, present only when the template approves egress |
| Entry | the image's default command starts an interactive shell or the agent; the browser terminal attaches to it |
| Must not need | root, a service-account token, DNS, or any credential in the environment |

An image is approved through the existing gate: SBOM, vulnerability scan and
signature evidence, then a digest on the allowlist
([image approval runbook](../operations/campus-image-approval.md)).

First agents to qualify against the contract: Codex CLI, Claude Code, and the
PHarness agent runtime.

### What boundary-level control means

For an outside agent the platform enforces: which image runs, what the sandbox
can reach, which models it may call, how much it may spend, how long it lives.
The platform does not see or approve the agent's individual file edits or shell
commands; those happen inside the sandbox.

The capability broker (`campus_capability.rs`) can authorize and audit each
tool call, but only for an agent that sends its tool calls to it. It is tested
and not wired into the API. It stays available for the PHarness agent runtime
and is not part of the promise for outside agents.

## Self-hosted install

One install serves one organization. Target shape:

- One Helm chart that installs the control plane, the broker, the egress proxy,
  the admission policies and the workspace namespaces. Today these are split
  between `deploy/helm/pharness-campus` here and `charts/campus-lab` and
  `charts/campus-idp` in the `lucas_engineering` repository.
- The customer's own OIDC provider. Today: any OIDC issuer works; Keycloak is
  only the test IdP ([OIDC runbook](../operations/campus-oidc-auth.md)).
- Stated cluster requirements: a gVisor RuntimeClass, a CNI that enforces
  NetworkPolicy, ValidatingAdmissionPolicy support.
- SQLite on one volume with documented backup and restore. A single replica is
  acceptable for a first release and must be stated as a limit.

## Not in scope now

- Hosted multi-tenant service.
- Tool-level approval for outside agents.
- SDLC stages, WorkItems, delivery and release verification (frozen).
- Hardware-isolated sessions beyond the existing gated Security profile.

## Naming

Code and wire identifiers still say Campus. The rename happens in two passes so
the deployed PoC keeps working.

| Today | Target | Pass |
| --- | --- | --- |
| `Campus*` Rust types, `campus_*` modules | `Workspace*`, neutral module names | 1: Rust only |
| institution / assignment / student authorities | organization / template / user | 1, with serde aliases for stored values |
| course, enrollment | group, membership | 2 |
| `/api/campus/*`, `PHARNESS_CAMPUS_*`, `campus_*` tables, `pharness.dev/campus-*` labels, `campus-lab-slot-N` namespaces | neutral names | 2: needs a migration and a coordinated GitOps change |

## Work sequence

| Step | Outcome | State |
| --- | --- | --- |
| W1 | This document; ASTRA frozen in the docs | done |
| W2 | Workspace core in its own crate, `pharness-workspaces`, with no SDLC dependency | done (pure move; names unchanged) |
| W3 | Rename pass 1 | next |
| W4 | Real inference: broker to gateway, credentials at install scope, metering | planned |
| W5 | Agent image contract v1 implemented and qualified with two outside agents | planned |
| W6 | Organization and user provider credentials | planned |
| W7 | Catalog in the database, admin API and page, per-session records | planned |
| W8 | Single install chart; workspace API as its own binary | planned |
| W9 | Rename pass 2 | planned, needs cluster access |

W4 and W5 together are the first point at which a user can bring an agent and
a provider. W8 is the first point at which another organization can install it.
