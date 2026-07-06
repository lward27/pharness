# Decisions

- Add durable `Release` records as the reviewable handoff after an approved `DeploymentIntent`.
- Keep V1 Releases non-executing. The default release JSON includes `execution.enabled = false` and records that Release is review state only in V1.
- Allow one current Release per DeploymentIntent in V1. Repeated create requests for the same DeploymentIntent return the existing release instead of creating duplicates.
- Require the parent DeploymentIntent to be `approved` before creating a Release.
- Default Release kind is `gitops_release`.
- Store release artifact fields directly: version, commit SHA, image digest, and rollback reference. These are policy and evidence inputs, not only descriptive JSON.
- Store target fields directly from the DeploymentIntent: target environment, target namespace, and Argo CD application.
- Use the status graph `proposed -> approved` or `proposed -> rejected`. Approved releases may be rejected later.
- A material ChangeSet revision that stales the current PipelineIntent and DeploymentIntent also marks the derived Release `stale`.
- Creating a Release for an approved DeploymentIntent that already has a stale Release re-proposes that same row in place.
- Expose Releases through `POST /api/releases/from-deployment-intent`, `GET /api/releases`, `GET /api/releases/:release_id`, and `POST /api/releases/:release_id/transition`, with matching CLI commands.
- Record `release.proposed`, `release.approved`, `release.rejected`, `release.stale`, and `release.reproposed` audit events.
- ChangeSet readiness reports Release state after the DeploymentIntent is approved. A missing, stale, or non-approved Release is a warning today, not a blocker, because V1 cluster mutation is still disabled.
- An approved Release can now produce one RegistryEvidence record for image verification review. RegistryEvidence is still non-executing in V1.
- Release creation now carries DeploymentIntent evidence into `release_json.deployment_evidence`.
  - Missing Argo evidence is explicit: `status = missing`, `release_ready = false`, `review_required = true`.
  - Attached Argo evidence keeps the observation id, artifact id, summarized sync/health fields, and raw evidence snapshot.
  - Evidence status remains separate from Release lifecycle status; V1 can propose Release records with cautionary evidence because no deployment mutation happens yet.
- Approved or proposed Releases can attach read-only observability observations through `POST /api/releases/:release_id/evidence` and `pharness-cli releases attach-evidence`.
  - V1 accepts Prometheus inventory/query observations and Loki log summary observations only.
  - Attached evidence is stored in `release_json.observability_evidence` with observation id, artifact id, compact summary, status, runtime-readiness flag, and observed resource identity.
  - Evidence attachment records `release.evidence_attached` and does not change the Release lifecycle status.
- Release evidence attachment may return a candidate Incident when the attached observability evidence is `attention_required`.
  - Incident creation is idempotent by release id plus observation id.
  - Prometheus alert inventory promotes as high severity when active alerts exist; other attention-required evidence starts at medium.
  - The response may also include a draft RemediationPlan with pending gates for the risky follow-up actions.
  - The Release lifecycle stays unchanged because V1 still treats this as review evidence, not an executing release controller.

# Backlog

- Add Release execution only after explicit DeploymentIntent execution and post-deploy verification capabilities exist.
- Add production policy gates for blast radius, sync windows, rollback confidence, LGTM status, image provenance, and database drift.
- Normalize richer LGTM evidence into typed resources once Prometheus, Loki, and future Tempo checks need stronger query/resource ownership than Release JSON can provide.
- Add release promotion and rollback flows once lower-environment execution is stable.
