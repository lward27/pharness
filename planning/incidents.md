# Decisions

- Add durable `Incident` candidate records as the first AIOps layer above observations.
- V1 incidents are read-only candidates derived from observations. They do not assign ownership, trigger remediation, mutate clusters, or open external tickets.
- Store incidents with `status`, `severity`, source observation id, run id, normalized resource identity, summary, and compact data JSON.
- Create incident candidates from Tekton `pipeline_run_analysis` observations when PipelineRun status, Deployment rollout status, Argo sync/health, or image alignment indicate risk.
- Expose incidents through `GET /api/incidents`, `GET /api/incidents/:incident_id`, `pharness-cli incidents list`, and `pharness-cli incidents get`.
- Candidate incidents can now produce conservative draft `RemediationPlan` records for operator review.

# Backlog

- Add incident deduplication across runs once repeated observation identity and time-window semantics are clear.
- Add lifecycle transitions such as acknowledged, linked, resolved, and ignored after operator workflows need them.
- Add direct capability incident derivation after direct capability calls have a durable request/session owner.
- Add incident noise controls before generating any executable remediation from candidate plans.
