# Decisions

- Add durable `Incident` candidate records as the first AIOps layer above observations.
- V1 incidents are read-only candidates derived from observations. They do not assign ownership, trigger remediation, mutate clusters, or open external tickets.
- Store incidents with `status`, `severity`, source observation id, run id, normalized resource identity, summary, and compact data JSON.
- Create incident candidates from Tekton `pipeline_run_analysis` observations when PipelineRun status, Deployment rollout status, Argo sync/health, or image alignment indicate risk.
- Create incident candidates from Release-attached observability evidence when Prometheus/Loki evidence is classified as `attention_required`.
- Create one candidate incident from an applied terminal WorkItem delivery
  failure. Its paired `delivery_failure` observation records the controller
  action, bounded failure code and summary, WorkItem/WorkPlan/run lineage,
  and explicit `automatic_retry=false`, `automatic_rollback=false`, and
  `mutation_performed=false` markers. It creates one deterministic read-only
  draft RemediationPlan with pending file, Git, pipeline, cluster, and
  production gates. A remediation WorkPlan can be derived only after the plan
  is explicitly proposed and approved with actor/reason evidence; the derived
  WorkPlan starts execution-disabled. Re-applying reconciliation to an already
  blocked WorkItem creates neither duplicate evidence, plan, nor retry.
- Release observability incident creation is deterministic by release id plus observation id, so re-attaching the same risky evidence returns the existing candidate instead of duplicating noise.
- Expose incidents through `GET /api/incidents`, `GET /api/incidents/:incident_id`, `pharness-cli incidents list`, and `pharness-cli incidents get`.
- Candidate incidents can now produce conservative draft `RemediationPlan` records for operator review.
- Release observability Incident candidates now produce idempotent draft `RemediationPlan` records with approval gates for file writes, pipeline mutation, cluster mutation, and production-impacting work.

# Backlog

- Add incident deduplication across runs once repeated observation identity and time-window semantics are clear.
- Add lifecycle transitions such as acknowledged, linked, resolved, and ignored after operator workflows need them.
- Add direct capability incident derivation after direct capability calls have a durable request/session owner.
- Add operator-reviewed transition and execution rules for delivery-failure
  remediation plans; the current controller creates only a draft and never
  dispatches it.
- Add incident noise controls before generating any executable remediation from candidate plans.
- Split Incident source-specific remediation builders into smaller modules once there are more than two durable sources.
