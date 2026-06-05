# Decisions

- Add `GET /api/approval-gates/summary` as the compact governance queue rollup for durable `ApprovalGate` records.
- Add `pharness-cli approval-gates summary` so Codex and operators can inspect gate pressure without fetching full gate payloads or writing ad hoc `jq`.
- Keep the summary descriptive only. It reports queue state and does not satisfy, waive, reject, approve tools, or execute remediation plans.
- Reuse the approval gate list filter dimensions except pagination: remediation plan, incident, run, status, gate kind, risk, normalized resource identity, and created-time filters.
- Group by status, gate kind, risk, stable age bucket, resource namespace, resource kind, resource name, incident id, and remediation plan id.

# Backlog

- Add assignee and required approver buckets after identity and role metadata exists.
- Add stale/deadline buckets after gates have due-time semantics.
- Add plan readiness rollups once WorkPlan/ChangeSet execution exists.
