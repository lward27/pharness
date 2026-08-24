# Decisions

- Add durable `ApprovalGate` records as the first machine-queryable gate layer above RemediationPlan draft JSON.
- Generate gates from RemediationPlan `approval_gates` when a candidate incident creates a draft plan.
- Store gates with remediation plan, incident, run, session, status, kind, order, risk level, normalized resource identity, and compact gate JSON.
- Expose gates through `GET /api/approval-gates`, `GET /api/approval-gates/:gate_id`, `pharness-cli approval-gates list`, and `pharness-cli approval-gates get`.
- Keep gates read-only in this slice. They describe required approvals; they do not decide tool approvals, mutate plans, or authorize execution.
- Add explicit lifecycle transitions: `satisfy`, `waive`, and `reject`. These update gate review status and emit audit events, but still do not authorize or execute work.
- Add compact approval gate queue summaries through `GET /api/approval-gates/summary` and `pharness-cli approval-gates summary`. Summaries group by status, gate kind, risk, age, resource identity, incident, and remediation plan.

# Backlog

- Link tool-level approvals to approval gates after plan execution exists.
- Add expiration only after gates have TTL or deadline semantics.
- Add gate assignment and required approver metadata after team/role identity exists.
- Add material-change invalidation so a changed plan reopens affected gates.
