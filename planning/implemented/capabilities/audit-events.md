# Decisions

- Add a separate durable `audit_events` store surface instead of overloading run events. Permission grant lifecycle records are control-plane audit facts and may exist outside a run.
- Store audit events with kind, actor, resource kind, resource id, optional run id, payload JSON, and creation time.
- Expose `GET /api/audit-events` for machine consumers, with filters for resource and run id.
- Add `pharness-cli audit-events` so smoke tests and Codex can inspect audit records without raw curl.
- Record `permission_grant.created`, `permission_grant.revoked`, and `permission_grant.stale` from the API permission-grant lifecycle. Creation accepts optional operator attribution through `created_by`; revocation uses `revoked_by`; stale-envelope invalidation uses the revision actor/reason.
- Record `permission_grant.used` from worker-persisted `policy.evaluated` events when a grant id is present.
- Keep the immediate audit payload JSON explicit and redundant enough for replay: grant id, source run/event, action, decision, and run scope where available.
- Record approval decisions as `approval.approved` and `approval.denied`. These audit records include approval id, run id, decision, approval kind, risk level, action kind, and run scope, but not the full reviewed action payload.
- Record direct capability outcomes as durable audit facts: `direct_capability.executed`, `direct_capability.failed`, and `direct_capability.denied`.
- Direct capability audit events include action kind, action id, policy decision, and an explicit `executed` flag, but not full capability arguments.
- Successful direct capability audit events store only a small result summary: source/resource, compact counts, and high-level status fields. Full Kubernetes, Prometheus, Loki, or Tekton payloads stay in the capability response and artifacts, not in the audit event.
- Failed direct capability audit events store a truncated error string. Denied direct capability audit events record `executed = false`.
- Registry direct capability audit summaries store only image identity, verification status, probe status, probe accessibility, and probe digest. They do not include registry credentials, response headers beyond compact metadata, or manifest bodies.
- Inspection-backed RegistryEvidence writes two audit facts: the direct `registry_inspect_image` capability outcome and the resulting `registry_evidence.proposed` or `registry_evidence.reproposed` lifecycle event. This keeps execution evidence separate from SDLC lifecycle evidence.
- Record PipelineIntent lifecycle and evidence events as `pipeline_intent.proposed`, `pipeline_intent.approved`, `pipeline_intent.rejected`, `pipeline_intent.stale`, `pipeline_intent.reproposed`, and `pipeline_intent.evidence_attached`.
- PipelineIntent audit payloads include the intent id, ChangeSet id, WorkPlan id, remediation path ids, status, intent kind, risk, target resource identity, and operator reason. They do not include future Tekton execution payloads beyond compact metadata.
- PipelineIntent evidence audit payloads include the Observation id, optional artifact id, compact evidence status, and observed resource identity. Full PipelineRunAnalysis JSON remains in the artifact/observation, not the audit event.
- Record DeploymentIntent lifecycle and evidence events as `deployment_intent.proposed`, `deployment_intent.approved`, `deployment_intent.rejected`, `deployment_intent.stale`, `deployment_intent.reproposed`, and `deployment_intent.evidence_attached`.
- DeploymentIntent audit payloads include the intent id, parent PipelineIntent id, ChangeSet id, WorkPlan id, remediation path ids, status, intent kind, risk, target environment, target namespace, Argo application, source resource identity, and operator reason. They do not include future Argo execution payloads beyond compact metadata.
- DeploymentIntent evidence audit payloads include the Observation id, optional artifact id, compact evidence status, deploy-readiness flag, and observed resource identity. Full Argo Application JSON remains in the artifact/observation, not the audit event.
- Record Release lifecycle events as `release.proposed`, `release.approved`, `release.rejected`, `release.stale`, and `release.reproposed`.
- Release audit payloads include the release id, DeploymentIntent id, PipelineIntent id, ChangeSet id, WorkPlan id, remediation path ids, status, release kind, risk, target environment, target namespace, Argo application, version, commit SHA, image digest, rollback ref, inherited deployment evidence status, release-readiness flag, and operator reason. They do not include future rollout execution payloads beyond compact metadata.
- Record Release evidence attachment as `release.evidence_attached`.
- Release evidence audit payloads include the Observation id, optional artifact id, compact evidence status, observation source/kind, and observed resource identity. Full Prometheus or Loki data remains in the Observation and artifact, not the audit event.
- Record RegistryEvidence lifecycle events as `registry_evidence.proposed`, `registry_evidence.verified`, `registry_evidence.rejected`, `registry_evidence.stale`, and `registry_evidence.reproposed`.
- RegistryEvidence audit payloads include the evidence id, Release id, upstream SDLC ids, status, risk, image identity, evidence source, verification status, and operator reason. They do not include registry credentials, scan blobs, or full registry response payloads.

# Backlog

- Add stronger audit taxonomy types before adding non-filesystem mutation capabilities.
- Decide whether audit events need signatures or hash chaining for V2 cluster deployment.
- Add API pagination once audit volume exceeds smoke-test scale.
- Add audit event redaction tests before persisting live registry scan summaries.
