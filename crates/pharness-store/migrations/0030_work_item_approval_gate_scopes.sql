-- Existing WorkItem delivery gates need the same immutable target boundary as
-- newly declared WorkPlans. A gate without this scope must never authorize a
-- later Git, Tekton, or Argo action.
UPDATE approval_gates
SET gate_json = json_set(
  gate_json,
  '$.scope',
  json_object(
    'work_item_id', work_item_id,
    'work_plan_id', (
      SELECT id
      FROM work_plans
      WHERE work_item_id = approval_gates.work_item_id
      ORDER BY created_at DESC, id DESC
      LIMIT 1
    ),
    'environment', (
      SELECT target_environment FROM work_items WHERE id = approval_gates.work_item_id
    ),
    'production_impacting', json(CASE WHEN (
      SELECT production_impacting FROM work_items WHERE id = approval_gates.work_item_id
    ) = 1 THEN 'true' ELSE 'false' END),
    'source_repository', (
      SELECT source_repo FROM work_items WHERE id = approval_gates.work_item_id
    ),
    'source_ref', (
      SELECT source_ref FROM work_items WHERE id = approval_gates.work_item_id
    ),
    'target_namespace', (
      SELECT target_namespace FROM work_items WHERE id = approval_gates.work_item_id
    ),
    'argo_application', (
      SELECT argo_application FROM work_items WHERE id = approval_gates.work_item_id
    ),
    'actions', json(CASE gate_kind
      WHEN 'git_mutation' THEN '["git_create_branch","git_commit","git_push","github_create_pull_request"]'
      WHEN 'pipeline_mutation' THEN '["tekton_create_pipeline_run"]'
      WHEN 'cluster_mutation' THEN '["tekton_create_pipeline_run","argocd_sync"]'
      WHEN 'production_impact' THEN '["production_action"]'
      ELSE '[]'
    END)
  )
)
WHERE work_item_id IS NOT NULL
  AND json_type(gate_json, '$.scope') IS NULL;
