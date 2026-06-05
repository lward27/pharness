CREATE TABLE remediation_plans (
  id TEXT PRIMARY KEY,
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
  plan_json TEXT NOT NULL DEFAULT '{}',
  created_at TEXT NOT NULL
);

CREATE INDEX idx_remediation_plans_incident
  ON remediation_plans(incident_id, created_at DESC);

CREATE INDEX idx_remediation_plans_status_created
  ON remediation_plans(status, created_at DESC);

CREATE INDEX idx_remediation_plans_run
  ON remediation_plans(run_id, created_at DESC);

CREATE INDEX idx_remediation_plans_resource_identity
  ON remediation_plans(resource_namespace, resource_kind, resource_name, created_at DESC);
