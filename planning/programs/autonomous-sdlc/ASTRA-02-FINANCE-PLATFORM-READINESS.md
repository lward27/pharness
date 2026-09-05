# ASTRA M02: Finance platform readiness

Status: accepted platform prerequisites on 2026-09-05. Downstream delivery gates remain open.
Evidence: [current M02 execution record](../../evidence/autonomous-sdlc/ASTRA-M02-FINANCE-PLATFORM-READINESS.md).
Authority: [approved program](ASTRA-00-PROGRAM.md).
Dependencies: M01. May proceed independently of M03.

## Objective and scope

Provide verified, finite hosting bindings and safe staging prerequisites for the two Finance applications.

The program's locked authority, budget, source identity, compatibility, and evidence
rules apply. Work on current verified main in an isolated `codex/` worktree.
Do not mark this milestone accepted because a dependency appears healthy.

## Implementation

1. Use only context lucas_engineering and the resolved kubeconfig. Change the authoritative lucas_engineering GitOps repository, preserving the dirty saved checkout.

2. Resolve Finance certificate renewal failures from cert-manager/issuer evidence; validate normal HTTPS. If a credential replacement is required, prepare the precise reference/configuration change and request the missing credential through an appropriate secret-management path.

3. Capture the running frontend imageID and pin that exact digest instead of latest. Record whether its original build source is known; do not invent provenance.

4. Create internal staging deployments in apps-staging for Deployment/yfinance-wrapper and Deployment/finance-frontend using existing manifests. Do not inherit production Ingress or production mutation permissions.

5. Bind exact source repositories, GitOps paths, Tekton pipelines, Argo applications, registry names, service ports, observability queries, and rollback baselines. Prepare a non-secret runtime-config.json ConfigMap mount for M11; existing frontend behavior remains unchanged until its application change.

6. Verify k3s-buildkit.tekton-pipelines.svc.cluster.local:12340, registry push/pull and immutable resolution, GitHub required-check/merge permissions, and relevant LGTM data. Staging browser probes run in an isolated cluster workload and cannot reach production mutation targets.

## Interfaces and compatibility

Finite native delivery bindings reuse existing repository, environment, pipeline, and deployment concepts. No generic provider/plugin framework.

Preserve immutable historical evidence, existing Finance data generation, and additive migration compatibility. Record any minimum compatible rollback version before enabling new writes.

## Tests and acceptance

- [x] Relevant Finance HTTPS endpoints validate with their normal certificate chain; origin HTTP health passes. Public automated probes receive Cloudflare HTTP 403, recorded separately.
- [x] Current running frontend digest is preserved in GitOps and remains healthy; no mutable production image reference remains for either target.
- [x] Staging resources render and pass server dry-run, then reconcile through Argo with production isolation demonstrated.
- [x] Finite coordinates and fresh app-scoped signals are recorded; separate worker checks passed. Runtime contract registration is M05; the real frontend Pipeline is M07. Missing frontend traces/request metrics remain explicit for M08.
- [x] Staging frontend completeness is explicitly pending M11; M02 does not claim its future runtime-config behavior exists.

Render Helm/Kustomize, inspect exact resource diff, server dry-run, apply via GitOps, then check Argo revision, imageID, HTTPS, endpoint reachability, and isolation. Never dump Secret values or workload environments.

## Deployment and recovery

Record exact changed resources, source revisions, validation, before/after identities,
and external effects. Keep managed cluster changes in authoritative GitOps sources.
Production runtime acceptance still requires a concrete human approval.

Revert only the milestone's GitOps paths to recorded values. Do not delete namespaces/PVCs or reset Finance data. Identify any incompatible change before applying it.

## Evidence and closeout

Write ASTRA-M02-FINANCE-PLATFORM-READINESS.md with sanitized target inventory, before/after image identities, certificate status, validation, and unresolved blockers.
Use `planning/evidence/autonomous-sdlc/` for milestone execution evidence unless an
existing assessment location is explicitly named. Include date, revisions, commands
without secrets, observed results, failures, limitations, and commit/release identities.
A test result and a deployed result are separate claims.

Review coverage: Platform prerequisites for F13.
Update the master ledger and this document only after its checks are evidenced.
Unmet criteria remain unchecked with a concrete reason and next action.

## Goal-mode execution prompt

Read ASTRA-00-PROGRAM.md and this milestone. Verify dependencies against current
evidence, inspect the affected implementation, execute the bounded changes above,
and run the specified meaningful checks. Preserve user work and all safety/identity
boundaries. Record results, commit the implementation and evidence, update the master
and finding ledger, then continue the next eligible milestone. If an external input is
missing, explain the exact blocker and continue independent work. Do not weaken a gate,
silently switch provider/budget, or claim unexecuted deployment or autonomous acceptance.

