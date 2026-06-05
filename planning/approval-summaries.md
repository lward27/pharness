# Decisions

- Add `GET /api/approvals/summary` as a compact machine-facing approval queue rollup.
- Use the same filter dimensions as approval listing: status, namespace, repo, branch, and production-impacting metadata.
- Support requested-time filters with `requested_after_ms` and `requested_before_ms`, matching approval listing semantics.
- Return grouped counts by status, approval kind, risk level, namespace, repo, branch, and production-impacting value.
- Return stable age buckets from `requested_at`: `lt_5m`, `5m_to_1h`, `1h_to_24h`, and `gte_24h`. Buckets are ordered by age so stale gates are visible without client-side sorting.
- Keep summaries payload-only. They do not include reviewed action JSON, preview diffs, transcripts, or full approval records.
- Add `pharness-cli approvals summary` so operator and Codex smoke tests do not need raw curl or ad hoc `jq` aggregation.

# Backlog

- Add actor/decider summaries after approval assignment exists.
- Add UI rendering later as a top-level approval queue panel, fed from the summary endpoint rather than client-side aggregation.
