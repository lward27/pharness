# Decisions

- Treat trusted envelopes as approval snapshots over a specific WorkPlan or ChangeSet state, not permanent capability grants.
- Mark active WorkPlan-scoped grants `stale` when the WorkPlan has a material revision. This also invalidates broader envelopes created before the plan changed.
- Mark active ChangeSet-scoped grants `stale` when a material ChangeSet revision changes the material hash. Non-material or hash-identical revisions do not stale the grant.
- Keep stale effects future-facing: new runs snapshot only active grants, while existing runs keep their persisted policy snapshot for reproducibility.
- Emit `permission_grant.stale` audit events with the revision actor and reason.

# Backlog

- Add status-gated trusted-envelope creation once WorkPlan and ChangeSet approval ownership is stable.
- Add a concise operator summary that groups stale gates, stale ChangeSets, and stale PermissionGrants for one revision.
- Consider an explicit `stale_at`, `stale_by`, and `stale_reason` schema if stale and revoked lifecycles need to diverge.
