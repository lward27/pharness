-- GitOps repository writes are distinct from source-code writes and Argo
-- reconciliation. Backfill the narrowly scoped gate for existing WorkItems
-- that already declare a GitOps target.
INSERT INTO approval_gates (
  id, work_item_id, remediation_plan_id, incident_id, session_id, run_id,
  status, gate_kind, gate_order, title, summary, risk_level,
  resource_namespace, resource_kind, resource_name, gate_json, created_at
)
SELECT
  'agate_' || wi.id || '_4_gitops_mutation',
  wi.id,
  NULL,
  NULL,
  wp.session_id,
  wp.run_id,
  'pending',
  'gitops_mutation',
  4,
  'Approve gitops mutation',
  'Approval required before creating a GitOps branch, commit, or pull request.',
  wp.risk_level,
  wp.resource_namespace,
  wp.resource_kind,
  wp.resource_name,
  json_object(
    'kind', 'gitops_mutation',
    'required_before', 'creating a GitOps branch, commit, or pull request',
    'scope', json_object(
      'work_item_id', wi.id,
      'work_plan_id', wp.id,
      'environment', wi.target_environment,
      'production_impacting', json(CASE WHEN wi.production_impacting = 1 THEN 'true' ELSE 'false' END),
      'source_repository', wi.source_repo,
      'source_ref', wi.source_ref,
      'gitops_repository', wi.gitops_repo,
      'gitops_ref', wi.gitops_ref,
      'target_namespace', wi.target_namespace,
      'argo_application', wi.argo_application,
      'actions', json('["git_create_branch","git_commit","git_push","github_create_pull_request"]')
    )
  ),
  CAST(unixepoch('subsec') * 1000 AS TEXT)
FROM work_items wi
JOIN work_plans wp ON wp.work_item_id = wi.id
WHERE wi.gitops_repo IS NOT NULL
  AND wi.gitops_ref IS NOT NULL
  AND NOT EXISTS (
    SELECT 1 FROM approval_gates gate
    WHERE gate.work_item_id = wi.id AND gate.gate_kind = 'gitops_mutation'
  );
