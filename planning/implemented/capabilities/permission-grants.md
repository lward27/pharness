# Decisions

- Add `PermissionGrant` as durable control-plane state before broadening trusted modes. This prevents autonomy from becoming hidden runtime behavior.
- Store grants in SQLite with subject, status, reason, scope JSON, policy JSON, expiry, and revocation metadata. The JSON fields keep the V1 schema flexible while the API shape remains explicit.
- Expose grants through machine-facing API routes:
  - `GET /api/permission-grants`
  - `POST /api/permission-grants`
  - `GET /api/permission-grants/:grant_id`
  - `POST /api/permission-grants/:grant_id/revoke`
- Add CLI commands so smoke testing does not require raw curl:
  - `permission-grants create`
  - `permission-grants list`
  - `permission-grants get`
  - `permission-grants revoke`
- Snapshot active, unexpired grants onto new runs. The run execution target records the policy environment and grants used for policy evaluation so the run is reproducible.
- Evaluate grants narrowly in the policy engine. A matching grant can convert an `ask` decision into `allow` only for local `write_file` and `patch_file` actions in the filesystem capability.
- Require grant scope environment to match the run policy environment. The V1 default is `local`; this keeps later lower-environment autonomy from becoming a blanket grant.
- Enforce namespace, repo, branch, and production-impacting grant scope when those fields are present. Empty lists mean unrestricted for that dimension; non-empty lists require an exact run-scope match. `production_impacting` requires an exact boolean match when set.
- Enforce WorkPlan and ChangeSet grant scope when `work_plan_ids` or `change_set_ids` are present. Empty lists mean unrestricted for that dimension; non-empty lists require an exact run-scope id match.
- Add trusted-envelope factory commands for WorkPlan and ChangeSet resources so operators do not need to hand-author grant scope JSON for common SDLC envelopes.
- Require trusted-envelope factories to target approved WorkPlans. ChangeSet trusted envelopes also require the parent WorkPlan and target ChangeSet to both be approved. Draft/proposed resources cannot mint grants.
- Emit the matching grant as `decision.grant_id` on allowed policy decisions. Machines should not parse policy prose to prove why an action was allowed.
- Record grant lifecycle and use in durable audit events: `permission_grant.created`, `permission_grant.stale`, `permission_grant.revoked`, and `permission_grant.used`.
- Use `supervised_autonomy`, rather than file-write trust, for typed delivery
  actions. The Git delivery authorizer mints a grant only for an approved
  WorkItem-backed ChangeSet with a current immutable delivery-plan artifact.
  Its scope has the exact Git capability/actions, development environment,
  repository, issued branch, WorkPlan, ChangeSet, plan-artifact id, and an
  explicit `production_impacting=false` boundary.
- Make Git delivery readiness inspectable without issuing a Git operation.
  `git_delivery_preflight` persists the exact plan, writer subject, matching
  grant (if any), and checks. It reports `dispatch_ready=false` until the
  separately scoped writer exists; authorization must never be represented as
  source-control execution.
- Accept optional `created_by` on grant creation and use it as the `permission_grant.created` audit actor. The grant row remains policy state; actor attribution belongs in the audit event.
- Do not let grants override denials. Secret-accessing, privileged, destructive, network, shell, registry, deployment, and production mutation paths remain gated by the base policy.
- Treat `expires_at` as Unix milliseconds for now. Invalid expiry values are rejected at create time or ignored during snapshotting if older data exists.

# Backlog

- Add an expired-grant audit path when grant expiry becomes an active reconciliation concern.
- Add a negative smoke test that creates a scoped local write grant, runs a mismatched namespace, and confirms the run pauses for approval.
- Add a negative smoke test that creates a ChangeSet-scoped trusted envelope, runs with a mismatched `--change-set-id`, and confirms the run pauses for approval.
- Decide whether grant revocation should affect already-created runs. The current snapshot model makes revocation affect future runs only.
- Add the separate Git writer identity and require it to revalidate the scoped
  delivery-plan artifact and grant immediately before every remote operation.
  The grant factory alone does not confer Git credentials or execute Git.
