# Decisions

- Add durable `WorkPlan` records as the first explicit handoff from review-only remediation planning toward executable SDLC orchestration.
- Create WorkPlans idempotently from `RemediationPlan` records. A remediation plan has at most one current WorkPlan in V1.
- Keep WorkPlans non-executable in this slice. `work_plan_json.execution.enabled` is always `false`, and creating a WorkPlan does not satisfy approval gates, approve tools, run pipelines, mutate files, or change cluster state.
- Store WorkPlans with remediation plan, incident, run, session, status, title, summary, risk, approval requirement, normalized resource identity, and structured WorkPlan JSON.
- Expose WorkPlans through `POST /api/work-plans/from-remediation-plan`, `GET /api/work-plans`, `GET /api/work-plans/:work_plan_id`, and matching CLI commands.
- Add explicit WorkPlan lifecycle transitions through `POST /api/work-plans/:work_plan_id/transition` and `pharness-cli work-plans transition`.
- Use the status graph `draft -> proposed -> approved -> executing -> completed`, with `blocked` for paused execution and `rejected` as a terminal review outcome. Revisions move the plan back to `draft`.
- Add WorkPlan revisions through `POST /api/work-plans/:work_plan_id/revise` and `pharness-cli work-plans revise`. A material revision increments `revision`, records actor/reason metadata, and marks prior satisfied or waived approval gates for the same remediation plan as `stale`.
- Keep approval gates attached to remediation plans for now. Do not add `work_plan_id` until ChangeSets exist, because the current gate contract still represents review state for the proposed remediation path.
- Expose `POST /api/work-plans/:work_plan_id/trusted-envelope` and `pharness-cli work-plans create-trusted-envelope`. The command creates a broader filesystem-only trusted write grant scoped to the WorkPlan id.

# Backlog

- Add richer WorkPlan readiness summaries now that ChangeSets are durable resources.
- Move gate ownership from remediation-plan-only to WorkPlan/ChangeSet-aware once gate queues need resource-specific targeting.
- Require approved WorkPlan status before trusted-envelope creation once WorkPlan review ownership is stable.
- Mark or revoke trusted envelopes as stale when a WorkPlan material revision changes after grant creation.
- Add execution only after plan approval, bounded trusted modes, and audit correlation are strong enough for lower-environment automation.
