# Decisions

- Add durable `RemediationPlan` drafts as the first read-only bridge from incident candidates toward autonomous SDLC remediation.
- Generate draft plans only from candidate incidents. Plans do not execute mutations, open tickets, notify humans, rerun pipelines, or change cluster state.
- Store plans with incident/run/session links, status, title, summary, risk level, approval requirement, normalized resource identity, and structured plan JSON.
- The initial plan JSON is deliberately conservative: read-only Tekton analysis, bounded log review, worktree proposal, and explicit approval gates before any file, pipeline, cluster, or production-impacting change.
- Expose plans through `GET /api/remediation-plans`, `GET /api/remediation-plans/:plan_id`, `pharness-cli remediation-plans list`, and `pharness-cli remediation-plans get`.
- Persist first-class `ApprovalGate` rows from the draft plan gates so future queues do not need to parse plan JSON.
- Hand off from RemediationPlan to WorkPlan before creating ChangeSets. RemediationPlan remains the read-only incident response draft; WorkPlan/ChangeSet carry reviewable SDLC execution intent.

# Backlog

- Add deduplication and supersession once repeated incidents can be correlated across runs.
- Add model-assisted plan expansion only after deterministic read-only drafts are stable enough to measure noise.
