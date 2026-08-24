# WorkItem Approval Gates

## Decisions

- WorkItem gates will be durable `ApprovalGate` records with an explicit
  `work_item_id`; they must not be represented by a missing incident ID alone.
  The same record may carry a WorkPlan, ChangeSet, PipelineIntent, or
  DeploymentIntent reference in its scoped gate JSON, but `work_item_id` is
  the root ownership boundary.
- WorkItem and incident gates are mutually exclusive origins. A gate has either
  `work_item_id` or the existing remediation-plan and incident lineage. This
  preserves simple query semantics and prevents an incident waiver from being
  reused for unrelated feature delivery.
- A newly declared WorkItem WorkPlan receives pending gates derived from its
  bounded delivery stages: `git_mutation`, `pipeline_mutation`, and
  `cluster_mutation`. Production-impacting WorkItems additionally receive
  `production_impact`. Local source writes remain governed by the existing
  scoped PermissionGrant/trusted-envelope mechanism rather than duplicating
  file-level prompts as plan gates.
- Material WorkPlan or ChangeSet revisions stale all satisfied or waived gates
  rooted at that WorkItem. Replanning never revives a stale gate; a new reviewed
  plan creates new gate evidence.
- WorkItem gate scopes bind the WorkItem and WorkPlan identifiers, target
  environment, production flag, source repository/ref, namespace, Argo
  application, and permitted typed delivery actions. Scope-less WorkItem
  gates never authorize a delivery operation; migration `0030` backfills the
  durable scope for gates created before this rule.
- Git-delivery preflight consumes only a satisfied or waived `git_mutation`
  gate matching that exact WorkItem/WorkPlan/source scope and the four typed
  writer actions. A gate is an authorization prerequisite, not an executor
  credential; the matching trusted execution envelope remains separately
  required.
- The initial migration-backed implementation creates `git_mutation`,
  `pipeline_mutation`, and `cluster_mutation` gates when a WorkItem WorkPlan
  is declared, with `production_impact` added for production-impacting intent.
  WorkPlan and material ChangeSet revisions stale satisfied or waived gates.
  Tekton preflight now requires satisfied, scope-matching WorkItem
  pipeline/cluster gates as well as the existing scoped trusted execution
  envelope. Git preflight now requires a matching `git_mutation` gate and
  exact writer grant. Approval-gate list and summary APIs, together with their
  CLI commands, filter by WorkItem and return a `by_work_item_id` summary
  bucket.

## Backlog

- Apply the scoped-gate matcher to the future GitOps/Argo runner. Git writer
  and Tekton preflight already require exact WorkItem gate scope.
- Add a UI grouping that distinguishes tool approvals, scoped trusted envelopes,
  incident remediation gates, and WorkItem delivery gates.
