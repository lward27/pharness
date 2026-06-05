# Decisions

- Add durable `ChangeSet` records as the reviewable source-change handoff from `WorkPlan` toward execution.
- Keep V1 ChangeSets non-executing. A ChangeSet stores proposed changes, diffs, rollback notes, artifact references, or future commit metadata in `change_set_json`, but it does not write files or apply patches by itself.
- Allow one current ChangeSet per WorkPlan in V1. Revisions update the same row, increment `revision`, recompute `material_hash`, and move status back to `draft`.
- Use deterministic SHA-256 material hashes over the structured ChangeSet JSON. Hash changes are the basis for stale approval detection.
- Expose ChangeSets through `POST /api/change-sets`, `GET /api/change-sets`, `GET /api/change-sets/:change_set_id`, `POST /api/change-sets/:change_set_id/revise`, and `POST /api/change-sets/:change_set_id/transition`, with matching CLI commands.
- Use the status graph `draft -> proposed -> approved -> applied`, with `rejected` terminal. `stale` is an internal invalidation state for superseded ChangeSets.
- A material ChangeSet revision marks prior satisfied or waived approval gates for the same remediation path as `stale`.
- A material WorkPlan revision marks the current draft/proposed/approved ChangeSet for that WorkPlan as `stale`.
- Expose `POST /api/change-sets/:change_set_id/trusted-envelope` and `pharness-cli change-sets create-trusted-envelope`. The command creates a filesystem-only trusted write grant scoped to the ChangeSet id and its parent WorkPlan id.

# Backlog

- Add ChangeSet file-level helpers once the diff shape settles. The current root JSON can carry `changes`, but the API does not yet index individual paths.
- Require approved ChangeSet status before trusted-envelope creation once the operator workflow is stable enough to enforce status gates.
- Mark or revoke trusted envelopes as stale when a ChangeSet material hash changes after grant creation.
- Add ChangeSet-to-PipelineIntent generation after pipeline intent schemas exist.
- Add commit metadata, branch metadata, and rollback provenance once pharness can apply or delegate source changes.
- Move approval gates from remediation-plan-only ownership to WorkPlan/ChangeSet-aware ownership once gate queues need resource-specific targeting.
