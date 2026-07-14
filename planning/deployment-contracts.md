# Deployment Contracts

## Decisions

- A DeploymentContract is durable operator policy for one exact Argo CD target:
  target environment, target namespace, and Application name. It is not a
  namespace-wide wildcard.
- The first schema permits only `{"operation":"sync","prune":false,
  "force":false}`. Rejecting prune and force now keeps their future semantics
  from being silently introduced by an executor implementation.
- Contracts begin active and can only retire. There can be only one active
  contract for an exact target. Retired contracts remain in the audit log and
  cannot authorize future deployment preflight.
- This slice adds no Argo CD mutation endpoint, Kubernetes RBAC, executor Job,
  or trusted envelope. A contract is a required safety prerequisite, not
  authorization to act.

## Backlog

- Add an approved DeploymentIntent preflight that requires exactly one matching
  active DeploymentContract, satisfied build evidence, and deployment approval
  gates before returning an Argo CD sync operation preview.
- Add a deployment-scoped supervised-autonomy envelope and a distinct
  `pharness-argo-runner` service account only with the preflight.
- Add atomic contract replacement after the first deployment executor exists;
  avoid replacing active policy until execution semantics are complete.
