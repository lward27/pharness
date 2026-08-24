# Decisions

- Add durable `Release` records as the reviewable handoff after an approved `DeploymentIntent`.
- Keep V1 Releases non-executing. The default release JSON includes `execution.enabled = false` and records that Release is review state only in V1.
- Allow one current Release per DeploymentIntent in V1. Repeated create requests for the same DeploymentIntent return the existing release instead of creating duplicates.
- Require the parent DeploymentIntent to be `approved` before creating a Release.
- Default Release kind is `gitops_release`.
- Store release artifact fields directly: version, commit SHA, image digest, and rollback reference. These are policy and evidence inputs, not only descriptive JSON.
- Store target fields directly from the DeploymentIntent: target environment, target namespace, and Argo CD application.
- Use the status graph `proposed -> approved` or `proposed -> rejected`. Approved releases may be rejected later. A Release reaches `completed` only through the typed post-sync verification endpoint; generic lifecycle transition cannot bypass verification.
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
- A WorkItem-backed DeploymentIntent can create a non-executing Release without
  fabricating remediation or incident lineage. Its durable provenance remains
  the inherited DeploymentIntent, PipelineIntent, ChangeSet, and WorkPlan.
- When the parent PipelineIntent has a current verified `pipeline_build_output`
  artifact, Release creation inherits its exact digest and stores a compact
  `release_json.build_output` link: artifact id, digest-pinned image reference,
  image URL, and reported source commit. An explicit Release digest must match
  that output. Untrusted or malformed build output cannot be adopted into a
  Release.
- This is build identity and source linkage only. It does not assert registry
  access, signature verification, SBOM presence, vulnerability posture, or
  deployment success.
- `POST /api/releases/:release_id/verify` and `pharness-cli releases verify` now
  run a read-only post-sync verification transaction for one WorkItem-backed
  dev Release.
  - It requires the current Argo sync execution to have a durable `completed`
    outcome, then reads only the exact declared Argo Application and Deployment.
  - It persists both typed observations and a `post_sync_verification` summary
    in Release JSON, along with `release.post_sync_verified` or
    `release.post_sync_attention_required` audit evidence.
  - `--complete` / `complete: true` is accepted only when the Application is
    `Synced` and `Healthy` and the declared Deployment rollout is `healthy`; it
    also requires an explicit audit reason. The operation cannot mutate Argo,
    Kubernetes resources, or secrets.
- An exact active DeploymentContract may require the bounded
  `post_sync_verification.prometheus_inventory` criterion. When set to
  `required`, the verifier performs its fixed compact Prometheus inventory
  read, persists the observation when available, and requires no unhealthy
  targets, problem rules, or alerts before Release completion. Missing or
  unavailable Prometheus evidence is durable `attention_required`, not an
  error path that could be mistaken for a completed release.
  - The verifier uses the immutable DeploymentContract id stored in the Argo
    execution receipt. It does not adopt a newer active contract after the
    sync; a retired, missing, or target-mismatched recorded contract requires
    a new reviewed sync.

# Backlog

- Add target-scoped Loki and trace criteria to the typed post-sync verifier;
  current optional contract criteria cover only the bounded Prometheus
  inventory, in addition to Argo and the declared Deployment rollout.
- Add production policy gates for blast radius, sync windows, rollback confidence, LGTM status, image provenance, and database drift.
- Normalize richer LGTM evidence into typed resources once Prometheus, Loki, and future Tempo checks need stronger query/resource ownership than Release JSON can provide.
- Require the verified build-output artifact for every WorkItem-backed dev
  release after a disposable live Tekton build-output smoke has proven the
  contract in the target cluster.
- Add release promotion and rollback flows once lower-environment execution is stable.
