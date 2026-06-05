# ADR 0001: Local-First Runtime, Cluster-Native Direction

Date: 2026-05-03

## Status

Accepted

## Context

Pharness starts as a local coding agent harness. The fastest useful V1 is a local Rust runtime that can inspect a repo, call Fireworks AI, execute a small set of tools, ask for approvals, and persist a replayable session.

The long-term target is different: most serious usage will happen in a Kubernetes cluster where production-app workflows can assume access to a registry, Tekton, Argo CD, a database operator, LGTM observability, and a RAG store.

## Decision

V1 remains local-first, but the core model will include stable nouns for future cluster-native operation:

- `ExecutionTarget` separates "what the agent wants to do" from "where the tool runs."
- `ResourceRef` and `ArtifactRef` make events and sessions point at durable resources beyond local files.
- `CapabilityKind` gives policy a shared vocabulary for local tools and future Kubernetes, registry, Tekton, Argo, database, observability, and RAG capabilities.

V1 will not implement direct cluster mutation tools. Cluster-shaped commands remain ordinary shell commands guarded by conservative policy. Future cluster operations should be typed capabilities with structured args, scoped credentials, preflight checks, approvals, audit events, and verification artifacts.

The long-term SDLC control plane should converge on Kubernetes CRDs rather than ad hoc database-only workflow rows. The planned CRD vocabulary is:

- `Agent`, `Skill`, `ToolServer`, `PermissionGrant`, `Workspace`
- `WorkItem`, `WorkPlan`, `ChangeSet`
- `PipelineIntent`, `PipelineRunAnalysis`, `DeploymentIntent`, `Release`
- `Observation`, `Incident`, `RemediationPlan`
- `ApprovalGate`, `AuditEvent`

These CRDs are not V1 implementation scope. They are a schema direction: current `Run`, `AgentEvent`, `ToolResult`, `ResourceRef`, `ArtifactRef`, approval, and audit data should remain generic and structured enough to map to those resources later.

## Consequences

- The first implementation stays small and useful.
- Early storage and event schemas can represent non-file resources later.
- The agent loop does not need to be rewritten when execution moves from local process to Kubernetes worker.
- Production delivery can grow through explicit capabilities instead of an unsafe pile of shell wrappers.
- The future cluster control plane can reconcile SDLC resources instead of depending on hidden UI state or one-off worker memory.

## Non-Goals

- No plugin system in V1.
- No MCP dependency in V1.
- No direct production deployment autonomy.
- No broad cluster service account.
- No hidden RAG memory injection.
