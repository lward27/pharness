# Deployment Contracts

This reference preserves the shipped legacy sync contract. The
[hosted SDLC program](../../programs/autonomous-sdlc/ASTRA-00-PROGRAM.md)
defines the current development direction. Source `19b0c55` contains schema-54
staging/production records and the durable staging GitOps foundation, with hosted
creation still disabled. The newer
[native baseline gate](../../evidence/autonomous-sdlc/ASTRA-M08-DURABLE-BASELINE-ADMISSION.md)
is implemented separately and requires a compatible API/worker release before use.

Hosted completion requires the exact deployed digest, functional behavior and
fresh application-scoped runtime evidence. Argo auto-sync is observed after the
authorized GitOps change; an extra manual sync receipt or healthy Prometheus
inventory cannot satisfy that contract. Production GitOps merge still requires
human approval. These requirements do not rewrite historical source-only outcomes.

## Legacy sync contract

- A DeploymentContract is durable operator policy for one exact Argo CD target:
  target environment, target namespace, and Application name. It is not a
  namespace-wide wildcard.
- The base schema permits only `{"operation":"sync","prune":false,
  "force":false}`. Rejecting prune and force now keeps their future semantics
  from being silently introduced by an executor implementation.
- A contract may opt into a narrow, bounded post-sync runtime criterion:
  `"post_sync_verification":{"prometheus_inventory":"required"}`. It
  permits no PromQL input and reads only Pharness's compact Prometheus targets,
  rules, and alerts inventory. A required inventory that is unavailable or
  reports unhealthy targets, problem rules, or alerts prevents Release
  completion and records `attention_required`; it never fails open.
- Omitting `post_sync_verification` keeps Prometheus inventory disabled for
  backwards-compatible contracts. This legacy inventory contract has no Loki
  or trace criteria; the hosted Finance path binds those signals to native
  application identity and observation windows instead.
- Post-sync verification reads the immutable `deployment_contract_id` recorded
  in the completed Argo execution receipt. A missing legacy id has no
  contract-backed runtime criterion; a missing, retired, or target-mismatched
  recorded contract blocks the Release until a new reviewed sync is run.
- Contracts begin active and can only retire. There can be only one active
  contract for an exact target. Retired contracts remain in the audit log and
  cannot authorize future deployment preflight.
- A contract is a required safety prerequisite, not authorization to act. The
  control plane now has a development-only DeploymentIntent trusted envelope,
  dry preflight, and an operator-invoked sync executor. The Helm chart keeps
  its `pharness-argo-runner` identity disabled by default and scopes it to
  explicit Application names. An active exact contract and matching grant are
  rechecked by both dispatch and the worker context route.

## Legacy executor boundaries

- The purpose-built `pharness-argo-runner` Job is only eligible after the
  existing preflight proves an exact active contract, satisfied build evidence,
  scoped `cluster_mutation` gate, and supervised envelope. For a WorkItem with
  a declared GitOps target, it must additionally have an exact observed GitOps
  merge artifact; the execution receipt binds that artifact id and merge SHA.
  Its Helm/RBAC
  configuration remains disabled until a disposable dev Application is
  explicitly named in the allowlist. Helm refuses to render an enabled runner
  with an empty allowlist so Kubernetes never sees a broad `applications` Role.
- The executor merge-patches only an Argo `operation.sync` request with
  `prune=false`; it does not use a force option, change source revision, or
  mutate arbitrary Kubernetes resources.
- Add atomic contract replacement after the first deployment executor exists;
  avoid replacing active policy until execution semantics are complete.
