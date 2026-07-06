# Decisions

- Add read-only SDLC flow endpoints rooted at either a WorkPlan or ChangeSet.
  - `GET /api/work-plans/:work_plan_id/flow` renders pre-ChangeSet planning state.
  - `GET /api/change-sets/:change_set_id/flow` renders the full source-change and downstream delivery chain.
- Keep flow as an aggregate view over existing durable records instead of adding a new persistence model.
- Include the core chain, readiness, related Incidents, RemediationPlans, ApprovalGates, and compact resource-scoped AuditEvents in one machine-facing JSON response.
- Expose the same aggregate through `pharness-cli work-plans flow --work-plan-id` and `pharness-cli change-sets flow --change-set-id`.
- Use deterministic Release-observability Incident ids to include attention-required Release evidence artifacts without adding another index.

# Backlog

- Add pagination or event windows if flow audit payloads become too large for real production histories.
- Add direct Observation and Artifact summaries to flow once the UI has a concrete evidence drilldown design.
