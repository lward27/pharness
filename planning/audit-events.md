# Decisions

- Add a separate durable `audit_events` store surface instead of overloading run events. Permission grant lifecycle records are control-plane audit facts and may exist outside a run.
- Store audit events with kind, actor, resource kind, resource id, optional run id, payload JSON, and creation time.
- Expose `GET /api/audit-events` for machine consumers, with filters for resource and run id.
- Add `pharness-cli audit-events` so smoke tests and Codex can inspect audit records without raw curl.
- Record `permission_grant.created` and `permission_grant.revoked` from the API permission-grant lifecycle. Creation accepts optional operator attribution through `created_by`; revocation uses `revoked_by`.
- Record `permission_grant.used` from worker-persisted `policy.evaluated` events when a grant id is present.
- Keep the immediate audit payload JSON explicit and redundant enough for replay: grant id, source run/event, action, decision, and run scope where available.
- Record approval decisions as `approval.approved` and `approval.denied`. These audit records include approval id, run id, decision, approval kind, risk level, action kind, and run scope, but not the full reviewed action payload.
- Record direct capability outcomes as durable audit facts: `direct_capability.executed`, `direct_capability.failed`, and `direct_capability.denied`.
- Direct capability audit events include action kind, action id, policy decision, and an explicit `executed` flag, but not full capability arguments.
- Successful direct capability audit events store only a small result summary: source/resource, compact counts, and high-level status fields. Full Kubernetes, Prometheus, Loki, or Tekton payloads stay in the capability response and artifacts, not in the audit event.
- Failed direct capability audit events store a truncated error string. Denied direct capability audit events record `executed = false`.

# Backlog

- Add stronger audit taxonomy types before adding non-filesystem mutation capabilities.
- Decide whether audit events need signatures or hash chaining for V2 cluster deployment.
- Add API pagination once audit volume exceeds smoke-test scale.
