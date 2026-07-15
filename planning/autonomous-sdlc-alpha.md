# Autonomous SDLC Alpha

## Decisions

- Treat a WorkItem as the durable root for a requested feature, bug fix, or
  operational outcome. It records the target source repository and ref,
  optional GitOps repository and ref, intended environment, acceptance
  criteria, retry budget, and lifecycle independent of incident response.
- Keep incident-derived WorkPlans valid, but allow a WorkPlan to be rooted in
  exactly one of a WorkItem or a RemediationPlan. A feature request must not
  be represented as a synthetic incident merely to satisfy legacy storage.
- Add Workspace as a durable provenance record, not a persistent source tree.
  It records the immutable source revision, requested branch, assigned run,
  and retention state while artifacts retain the meaningful diff and output.
- Keep this source pass non-mutating outside the repository. No GitHub token,
  Git writer, Argo runner, database identity, or deployment RBAC is added.
  Those capabilities require separately named external targets and approval.
- The first autonomous delivery target remains an isolated development
  workload. Production promotion, database mutation, and autonomous merge
  stay blocked behind later typed policy and verification work.
- The WorkPlan lineage migration was exercised against a SQLite database at
  the prior migration level with an existing remediation-backed WorkPlan. It
  preserves that legacy lineage while enabling WorkItem-backed plans.

## Backlog

- Add an isolated code-runner workspace provisioner that clones a WorkItem's
  configured source ref and records the resolved commit before model work.
- Derive a ChangeSet from a real workspace Git diff and connect it to the
  WorkItem lineage; do not use user-supplied JSON as the final source-change
  provenance.
- Add a dedicated Git writer and GitHub PR capability after credentials,
  repository allowlists, branch protections, and pull-request semantics are
  explicitly configured.
- Add DeploymentIntent preflight and a separate Argo runner only after GitOps
  revision provenance is available.
