CREATE TABLE change_sets (
  id TEXT PRIMARY KEY,
  work_plan_id TEXT NOT NULL REFERENCES work_plans(id),
  remediation_plan_id TEXT NOT NULL REFERENCES remediation_plans(id),
  incident_id TEXT NOT NULL REFERENCES incidents(id),
  session_id TEXT NOT NULL REFERENCES sessions(id),
  run_id TEXT REFERENCES runs(id),
  status TEXT NOT NULL,
  title TEXT NOT NULL,
  summary TEXT NOT NULL,
  risk_level TEXT NOT NULL,
  material_hash TEXT NOT NULL,
  revision INTEGER NOT NULL DEFAULT 1,
  resource_namespace TEXT,
  resource_kind TEXT,
  resource_name TEXT,
  change_set_json TEXT NOT NULL DEFAULT '{}',
  created_at TEXT NOT NULL,
  updated_at TEXT,
  status_changed_at TEXT,
  status_changed_by TEXT,
  status_reason TEXT
);

CREATE UNIQUE INDEX idx_change_sets_work_plan
  ON change_sets(work_plan_id);

CREATE INDEX idx_change_sets_status_created
  ON change_sets(status, created_at DESC);

CREATE INDEX idx_change_sets_run
  ON change_sets(run_id, created_at DESC);

CREATE INDEX idx_change_sets_remediation_plan
  ON change_sets(remediation_plan_id, created_at DESC);

CREATE INDEX idx_change_sets_incident
  ON change_sets(incident_id, created_at DESC);

CREATE INDEX idx_change_sets_resource_identity
  ON change_sets(resource_namespace, resource_kind, resource_name, created_at DESC);
