# Decisions

- Make approvals first-class operator resources, not only run-scoped side effects.
- Keep the existing `POST /api/runs/:run_id/approvals` route for compatibility.
- Add approval-id routes for machine consumers:
  - `GET /api/approvals/:approval_id`
  - `POST /api/approvals/:approval_id/approve`
  - `POST /api/approvals/:approval_id/deny`
- Keep approval-id approve/deny request bodies small: reviewer and reason only. The decision is already represented by the route.
- Refuse approval-id decisions unless the target approval is still pending and is the current pending approval for its run. This prevents stale approval cards from deciding a newer gate.
- Extend the CLI so `approvals approve` and `approvals deny` accept either `--run-id` or `--approval-id`, with exactly one required.
- Add approval queue filters for run scope: namespace, repo, branch, and production-impacting metadata.
- Add basic approval queue pagination with `limit` and `offset`. Responses include `count`, `limit`, and `offset` alongside approval rows.
- Add requested-time filters to approval list and summary queries: `requested_after_ms` and `requested_before_ms`. They use Unix epoch milliseconds so machine callers can ask for stale or fresh approval gates without fetching the whole queue.
- Persist approval preview JSON on the approval row. For `write_file` and `patch_file`, the preview contains a best-effort generated diff, byte counts, target path, and explicit `ok` or `error` status. This makes approval review durable and machine-readable instead of recomputing from later filesystem state.
- Secret-shaped approval targets do not get diff previews. They return an error preview so review surfaces can explain why the diff is absent without exposing sensitive content.
- Add compact approval queue summaries through `GET /api/approvals/summary` and `pharness-cli approvals summary`. Summaries reuse the list filters and group counts by status, kind, risk level, and run scope.
- Approval summaries include stable age buckets from approval `requested_at`: `lt_5m`, `5m_to_1h`, `1h_to_24h`, and `gte_24h`.

# Backlog

- Consider a dedicated `POST /api/approvals/:approval_id/decision` route only if clients need a single method-dispatched endpoint.
- Add approval-preview rendering to the future web UI, using the persisted JSON rather than rereading local files.
