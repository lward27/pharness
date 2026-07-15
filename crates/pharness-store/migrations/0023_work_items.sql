CREATE TABLE work_items (
  id TEXT PRIMARY KEY,
  status TEXT NOT NULL,
  title TEXT NOT NULL,
  intent TEXT NOT NULL,
  acceptance_criteria_json TEXT NOT NULL DEFAULT '[]',
  source_repo TEXT NOT NULL,
  source_ref TEXT NOT NULL,
  gitops_repo TEXT,
  gitops_ref TEXT,
  target_environment TEXT NOT NULL,
  target_namespace TEXT,
  argo_application TEXT,
  production_impacting INTEGER NOT NULL DEFAULT 0,
  max_attempts INTEGER NOT NULL,
  max_elapsed_seconds INTEGER NOT NULL,
  attempt_count INTEGER NOT NULL DEFAULT 0,
  current_run_id TEXT REFERENCES runs(id),
  created_by TEXT,
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL,
  status_changed_at TEXT NOT NULL,
  status_changed_by TEXT,
  status_reason TEXT
);

CREATE INDEX idx_work_items_status_created
  ON work_items(status, created_at DESC);

CREATE INDEX idx_work_items_target
  ON work_items(target_environment, target_namespace, argo_application, created_at DESC);

CREATE TABLE workspaces (
  id TEXT PRIMARY KEY,
  work_item_id TEXT NOT NULL REFERENCES work_items(id),
  run_id TEXT REFERENCES runs(id),
  status TEXT NOT NULL,
  source_repo TEXT NOT NULL,
  source_ref TEXT NOT NULL,
  resolved_commit TEXT,
  branch TEXT,
  retention_status TEXT NOT NULL,
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL,
  status_changed_at TEXT NOT NULL,
  status_changed_by TEXT,
  status_reason TEXT
);

CREATE INDEX idx_workspaces_work_item_created
  ON workspaces(work_item_id, created_at DESC);

CREATE INDEX idx_workspaces_run
  ON workspaces(run_id, created_at DESC);
