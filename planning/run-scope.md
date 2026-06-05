# Decisions

- Add `RunScope` as explicit SDLC metadata on run creation. It carries namespace, repo, branch, and whether the run is production-impacting.
- Persist run scope inside the run execution target and expose it through run responses and queued events. This makes scope replayable before it becomes an enforcement input.
- Add CLI flags for run scope metadata: `--namespace`, `--repo`, `--branch`, and `--production-impacting`.
- Treat run scope as target metadata, not authorization by itself. It does not authorize writes, cluster mutation, deployment, registry writes, or production-impacting behavior unless a separate approval or matching permission grant allows the action.
- Persist run scope on approval records. Approval decisions need to be auditable against the target envelope that existed when the approval was requested, not reconstructed from mutable run state later.
- Include run scope in `approval.required`, `approval.decided`, and `run.resumed` events. Operators and future policy reviewers should see the same namespace/repo/branch/production-impacting context in the event stream that Codex sees through the API.
- Use run scope as an enforcement input for permission grants. Grants with namespace, repo, branch, or production-impacting restrictions only match runs carrying the same scope values.
- Add `--work-plan-id` and `--change-set-id` run flags. Trusted envelopes with WorkPlan or ChangeSet restrictions only match runs carrying the same SDLC resource ids.
- Normalize empty run scope as absent. API run responses omit/return `null` for empty scope, result/event payloads serialize `run_scope: null`, and non-empty scopes remain structured objects. This prevents machine clients from treating default `production_impacting=false` as an explicit target envelope.

# Backlog

- Add validation for namespace/repo/branch patterns once the accepted value shapes are stable.
- Add approval summary formatting that highlights scope concisely for human review surfaces.
- Consider deriving default repo and branch from git metadata during run creation, but keep operator-provided values authoritative.
- Decide whether `production_impacting` should be inferred from environment tier, namespace naming, Argo app metadata, or explicit operator input.
