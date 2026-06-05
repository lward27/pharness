# Decisions

- Treat approval fatigue as a core product risk, not a UI annoyance. The long-term goal is bounded autonomy with audit, not endless manual approval cards.
- Keep fine-grained audit events for every material action. Human-facing summaries can compress large work streams, but they must reference durable run, event, approval, artifact, work plan, and change set records.
- Evolve approvals from individual tool calls toward approving a bounded `WorkPlan`, `ChangeSet`, `PipelineIntent`, `DeploymentIntent`, or `Release`. Once approved, lower-level actions inside that envelope can proceed without repeated prompts.
- Add trusted modes later as explicit policy modes, not hidden bypasses. Trust should be scoped by environment, namespace, capability kind, repo, branch, time window, and blast radius.
- Keep production mutation behind stricter gates by default. Lower environments and explicitly trusted namespaces can allow no-approval execution for well-understood capability classes once confidence is earned.
- Let Codex or another orchestrator make judgment calls about alignment with operator intent, but keep pharness as the policy-enforcing control plane. Model judgment can recommend allow/ask/deny; the runtime must still enforce grants, denials, secret rules, and audit.
- Design future agents as workers under an orchestrated SDLC control plane: the operator supplies intent, the orchestrator decomposes and supervises, workers execute continuously, and pharness records state, events, approvals, artifacts, and outcomes.
- Start the trusted-mode implementation as a narrow V1 policy seam: config defines the default `SafetyPolicy`, `POST /api/runs` may override only `policy_mode`, the selected policy is persisted on the run execution target, and the worker enforces the persisted policy when running or resuming.
- Keep `trusted_writes` limited to local filesystem/file-write autonomy for now. It must not auto-approve network, privileged, secret-accessing, destructive, registry, deployment, or production mutation paths.
- Evaluate active `PermissionGrant` records as a narrow autonomy envelope. A matching grant can allow local `write_file` and `patch_file`; it cannot override denials or authorize shell, network, privileged, secret, destructive, registry, deployment, or production mutation paths.
- Snapshot grants onto new runs instead of re-reading mutable grant state mid-run. This makes a run reproducible, but revocation currently affects future runs only.
- Require grant scope environment to match the run policy environment. This is the first environment-aware trust boundary; namespace/repo/branch matching should follow when those fields are explicit on runs and typed capabilities.
- Add run scope metadata for namespace, repo, branch, and production impact. It is persisted, observable, and used as an enforcement input for matching permission grants.
- Extend run scope with optional `work_plan_id` and `change_set_id`. Trusted envelopes can now be scoped to SDLC resources instead of only repo/branch/namespace metadata.
- Add trusted-envelope factory endpoints for `WorkPlan` and `ChangeSet` resources. They create audited `PermissionGrant` records with `trusted_writes`, filesystem-only capability scope, `write_file`/`patch_file` actions, medium maximum risk, explicit environment, optional namespace/repo/branch, and default `production_impacting = false`.
- Prefer ChangeSet-scoped trusted envelopes for source-change execution. They carry both `work_plan_ids` and `change_set_ids`; WorkPlan envelopes are broader and should be used only when there is no concrete ChangeSet yet.
- Keep trusted-envelope creation as an explicit operator/API action in V1. Creating an envelope does not mark a WorkPlan or ChangeSet approved, does not execute changes, and does not mutate cluster state.

# Backlog

- Rename or extend `trusted_writes` into environment-aware modes such as `trusted_local`, `trusted_lower_env`, `supervised_autonomy`, and `break_glass`, with exact allow/ask/deny behavior for each capability.
- Require approved WorkPlan/ChangeSet status before trusted-envelope creation once status ownership and operator workflow settle.
- Invalidate or warn on trusted envelopes when a WorkPlan or ChangeSet changes materially after grant creation.
- Extend trusted envelopes beyond local file edits only after typed `PipelineIntent`, `DeploymentIntent`, and environment-aware mutation policies exist.
- Add drift detection between approved plan and executed actions. If an agent leaves the approved envelope, pharness should pause and require a new approval.
- Add summary artifacts that roll up many events into an operator-readable narrative while preserving links to exact event IDs, tool calls, SQL rows, diffs, logs, and policy decisions.
- Add environment-aware defaults: no approval for read-only inspection; limited no-approval writes in local/dev; approval for staging deploys until confidence increases; explicit approval for production mutation.
- Add higher-level envelope-used summaries that group many grant-used audit events by WorkPlan, ChangeSet, run, and time window.
- Add budget and circuit-breaker controls for autonomous work: max runtime, max cost, max retries, max changed files, max deployment attempts, and automatic pause on repeated failures.
- Add an orchestrator review step before risky gates: Codex evaluates whether the proposed action still matches the original intent, but pharness records the judgment and enforces the final policy decision.
