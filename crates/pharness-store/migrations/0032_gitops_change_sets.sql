-- GitOps repository mutations have different provenance from source-code
-- changes. Keep the reviewed manifest update as a first-class resource.
CREATE TABLE gitops_change_sets (
  id TEXT PRIMARY KEY,
  work_item_id TEXT NOT NULL REFERENCES work_items(id),
  work_plan_id TEXT NOT NULL REFERENCES work_plans(id),
  source_change_set_id TEXT NOT NULL REFERENCES change_sets(id),
  pipeline_intent_id TEXT NOT NULL REFERENCES pipeline_intents(id),
  deployment_intent_id TEXT NOT NULL REFERENCES deployment_intents(id),
  gitops_update_plan_artifact_id TEXT NOT NULL REFERENCES artifacts(id),
  session_id TEXT NOT NULL REFERENCES sessions(id),
  run_id TEXT NOT NULL REFERENCES runs(id),
  status TEXT NOT NULL,
  title TEXT NOT NULL,
  summary TEXT NOT NULL,
  risk_level TEXT NOT NULL,
  material_hash TEXT NOT NULL,
  revision INTEGER NOT NULL DEFAULT 1,
  gitops_repo TEXT NOT NULL,
  gitops_ref TEXT NOT NULL,
  head_branch TEXT NOT NULL,
  kustomization_path TEXT NOT NULL,
  image_name TEXT NOT NULL,
  image_ref TEXT NOT NULL,
  gitops_change_set_json TEXT NOT NULL DEFAULT '{}',
  created_at TEXT NOT NULL,
  updated_at TEXT,
  status_changed_at TEXT,
  status_changed_by TEXT,
  status_reason TEXT,
  UNIQUE(pipeline_intent_id)
);

CREATE INDEX idx_gitops_change_sets_work_item
  ON gitops_change_sets(work_item_id, created_at DESC);
CREATE INDEX idx_gitops_change_sets_deployment_intent
  ON gitops_change_sets(deployment_intent_id, created_at DESC);
CREATE INDEX idx_gitops_change_sets_status_created
  ON gitops_change_sets(status, created_at DESC);
