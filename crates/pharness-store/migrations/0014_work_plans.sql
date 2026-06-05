CREATE TABLE work_plans (
  id TEXT PRIMARY KEY,
  remediation_plan_id TEXT NOT NULL REFERENCES remediation_plans(id),
  incident_id TEXT NOT NULL REFERENCES incidents(id),
  session_id TEXT NOT NULL REFERENCES sessions(id),
  run_id TEXT REFERENCES runs(id),
  status TEXT NOT NULL,
  title TEXT NOT NULL,
  summary TEXT NOT NULL,
  risk_level TEXT NOT NULL,
  requires_approval INTEGER NOT NULL DEFAULT 1,
  resource_namespace TEXT,
  resource_kind TEXT,
  resource_name TEXT,
  work_plan_json TEXT NOT NULL DEFAULT '{}',
  created_at TEXT NOT NULL
);

CREATE UNIQUE INDEX idx_work_plans_remediation_plan
  ON work_plans(remediation_plan_id);

CREATE INDEX idx_work_plans_incident
  ON work_plans(incident_id, created_at DESC);

CREATE INDEX idx_work_plans_status_created
  ON work_plans(status, created_at DESC);

CREATE INDEX idx_work_plans_run
  ON work_plans(run_id, created_at DESC);

CREATE INDEX idx_work_plans_resource_identity
  ON work_plans(resource_namespace, resource_kind, resource_name, created_at DESC);
