# Decisions

- Add durable `PipelineIntent` records as the reviewable bridge from an approved `ChangeSet` to future Tekton execution.
- Keep V1 PipelineIntents non-executing. The default intent JSON includes `execution.enabled = false` and records that PipelineIntent is review state only in V1.
- Allow one current PipelineIntent per ChangeSet in V1. Repeated create requests for the same ChangeSet return the existing intent instead of creating duplicates.
- A material ChangeSet revision with a changed material hash marks the current PipelineIntent as `stale`.
- Creating a PipelineIntent for a ChangeSet that already has a stale intent re-proposes that same row in place with intent JSON derived from the current ChangeSet material hash.
- Require both the parent WorkPlan and the ChangeSet to be `approved` before creating a PipelineIntent. Draft or proposed source changes should not create execution intent.
- Default PipelineIntent kind is `tekton_build_test_package`, with planned tasks `test`, `build`, and `package`.
- Use the status graph `proposed -> approved` or `proposed -> rejected`. Approved intents may be rejected later, and source changes can internally move any current intent to `stale`.
- Expose PipelineIntents through `POST /api/pipeline-intents/from-change-set`, `GET /api/pipeline-intents`, `GET /api/pipeline-intents/:pipeline_intent_id`, and `POST /api/pipeline-intents/:pipeline_intent_id/transition`, with matching CLI commands.
- Expose PipelineIntent evidence attachment through `POST /api/pipeline-intents/:pipeline_intent_id/evidence` and `pharness-cli pipeline-intents attach-evidence`.
- Only `tekton/pipeline_run_analysis` observations can be attached as PipelineIntent evidence in V1.
- Store attached evidence inside `intent_json.evidence` instead of adding columns while the CRD shape is still settling.
  - `intent_json.evidence.status` is execution evidence status, not lifecycle status.
  - PipelineIntent lifecycle status remains `approved`, `proposed`, `rejected`, or `stale`.
  - PipelineRun success with no failed tasks is `satisfied` only when Argo and image-alignment signals do not need attention.
  - Registry mismatch, Argo drift, Argo unhealthy, or failed task evidence records `attention_required`.
- Record `pipeline_intent.proposed`, `pipeline_intent.approved`, `pipeline_intent.rejected`, `pipeline_intent.stale`, `pipeline_intent.reproposed`, and `pipeline_intent.evidence_attached` audit events.
- ChangeSet readiness reports PipelineIntent state. A missing, stale, or non-approved PipelineIntent is a warning today, not a blocker, because V1 trusted-envelope execution is still filesystem-only.
- An approved PipelineIntent can now produce one DeploymentIntent for review. DeploymentIntent creation is still non-executing in V1.
- When a material ChangeSet revision marks a PipelineIntent stale, the derived DeploymentIntent is marked stale as well. Re-proposing the approved PipelineIntent can then re-propose the same DeploymentIntent row.

# Backlog

- Add `PipelineIntent` execution only as a separate approved typed capability. Do not hide Tekton mutation behind shell execution.
- Add Tekton PipelineRun template/rendering once the execution contract is stable.
- Add status transitions for execution once worker-side Tekton creation exists.
- Consider requiring approved PipelineIntent as a blocker for any future cluster-backed execution mode.
- Promote `intent_json.evidence` into typed columns only after the future `PipelineIntent` CRD status shape stabilizes.
