# Decisions

- Add `GET /api/runs` as the durable run queue/listing endpoint. Single-run fetch remains `GET /api/runs/:id`.
- Add `GET /api/runs/summary` as the compact run queue rollup endpoint.
- Reuse the same `RunResponse` shape for single-run and list responses, including run timestamps, optional scope, and result JSON.
- Add `pharness-cli runs list`, `pharness-cli runs summary`, and `pharness-cli runs cancel` so Codex and operators can inspect and stop runs without raw curl.
- Support filters by run status, run scope (`namespace`, `repo`, `branch`, `production_impacting`), and start time (`started_after_ms`, `started_before_ms`).
- Use Unix epoch milliseconds for run time filters. This keeps the API machine-facing and avoids ambiguous date parsing in the control plane.
- Keep pagination simple with `limit` and `offset`, clamped server-side to the same 200-row ceiling used for approvals.
- Summary responses group by status, age bucket, namespace, repo, branch, and production-impacting value. Age buckets match approvals: `lt_5m`, `5m_to_1h`, `1h_to_24h`, and `gte_24h`.
- `runs cancel --with-events` returns the cancelled run plus durable events, including `run.cancelled`, so cancellation smoke tests do not need curl.

# Backlog

- Add `session_id` filtering if multi-session run queues become noisy.
- Add started/finished duration buckets if stale running runs need the same treatment as stale approvals.
