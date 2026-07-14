# Decisions

- Add durable `DeploymentIntent` records as the reviewable bridge from an approved `PipelineIntent` to future Argo deployment execution.
- Keep V1 DeploymentIntents non-executing. The default intent JSON includes `execution.enabled = false` and records that DeploymentIntent is review state only in V1.
- Allow one current DeploymentIntent per PipelineIntent in V1. Repeated create requests for the same PipelineIntent return the existing intent instead of creating duplicates.
- Require the parent PipelineIntent to be `approved` before creating a DeploymentIntent. Build/test/package intent approval is the prerequisite for deployment intent review.
- Default DeploymentIntent kind is `argo_sync_deploy`.
- Store deployment target fields directly: target environment, target namespace, and Argo CD application. These are policy inputs, not just descriptive JSON.
- Use the status graph `proposed -> approved` or `proposed -> rejected`. Approved intents may be rejected later.
- A material ChangeSet revision that stales the current PipelineIntent also marks the derived DeploymentIntent `stale`.
- Creating a DeploymentIntent for an approved PipelineIntent that already has a stale DeploymentIntent re-proposes that same row in place.
- Expose DeploymentIntents through `POST /api/deployment-intents/from-pipeline-intent`, `GET /api/deployment-intents`, `GET /api/deployment-intents/:deployment_intent_id`, and `POST /api/deployment-intents/:deployment_intent_id/transition`, with matching CLI commands.
- Record `deployment_intent.proposed`, `deployment_intent.approved`, `deployment_intent.rejected`, `deployment_intent.stale`, and `deployment_intent.reproposed` audit events.
- ChangeSet readiness reports DeploymentIntent state after the PipelineIntent is approved. A missing, stale, or non-approved DeploymentIntent is a warning today, not a blocker, because V1 cluster mutation is still disabled.
- An approved DeploymentIntent can now produce one Release for review. Release creation is still non-executing in V1.
- When a material ChangeSet revision marks a DeploymentIntent stale, the derived Release is marked stale as well. Re-proposing the approved DeploymentIntent can then re-propose the same Release row.
- DeploymentIntent creation now carries the parent PipelineIntent evidence into `intent_json.pipeline_evidence`.
  - Missing evidence is explicit: `status = missing`, `deploy_ready = false`, `review_required = true`.
  - Attached evidence keeps the observation id, artifact id, summarized Tekton/Argo/image-alignment fields, and the raw evidence snapshot.
  - `attention_required`, `running`, `failed`, and `unknown` evidence do not block V1 DeploymentIntent proposal, but they are machine-readable review signals and audit context.
- DeploymentIntent audit events include upstream pipeline evidence status and deploy-readiness state.
- Approved DeploymentIntents can now attach Argo CD Application observations through `POST /api/deployment-intents/:deployment_intent_id/evidence` and `pharness-cli deployment-intents attach-evidence`.
  - Only `argocd` Application observations are accepted in V1.
  - Attached evidence is stored in `intent_json.deployment_evidence`.
  - `Synced` and `Healthy` evidence records `status = satisfied`, `deploy_ready = true`, and `review_required = false`.
  - Out-of-sync or unhealthy evidence records `attention_required`; missing fields record `unknown`.
  - Lifecycle status remains separate from evidence status.

# Backlog

- DeploymentContracts now carry exact target policy for a future typed Argo
  capability. They currently permit only a non-pruning, non-forced `sync`
  shape and do not grant execution authority.
- Add DeploymentIntent execution only as a separate approved typed Argo capability. Do not hide Argo mutation behind shell execution.
- Add production policy gates for blast radius, sync windows, protected namespaces, and rollback evidence before any production-impacting DeploymentIntent can execute.
- Add Argo preview/diff evidence before approving deploy intent.
- Promote pipeline evidence warnings to blockers once real deployment execution exists, especially for production-impacting DeploymentIntents.
- Promote deployment evidence warnings to blockers once real release or deployment execution exists, especially for production-impacting Release records.
