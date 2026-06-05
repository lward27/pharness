CREATE TABLE approval_gates (
  id TEXT PRIMARY KEY,
  remediation_plan_id TEXT NOT NULL REFERENCES remediation_plans(id),
  incident_id TEXT NOT NULL REFERENCES incidents(id),
  session_id TEXT NOT NULL REFERENCES sessions(id),
  run_id TEXT REFERENCES runs(id),
  status TEXT NOT NULL,
  gate_kind TEXT NOT NULL,
  gate_order INTEGER NOT NULL DEFAULT 0,
  title TEXT NOT NULL,
  summary TEXT NOT NULL,
  risk_level TEXT NOT NULL,
  resource_namespace TEXT,
  resource_kind TEXT,
  resource_name TEXT,
  gate_json TEXT NOT NULL DEFAULT '{}',
  created_at TEXT NOT NULL
);

CREATE INDEX idx_approval_gates_plan
  ON approval_gates(remediation_plan_id, gate_order ASC);

CREATE INDEX idx_approval_gates_status_created
  ON approval_gates(status, created_at DESC);

CREATE INDEX idx_approval_gates_run
  ON approval_gates(run_id, created_at DESC);

CREATE INDEX idx_approval_gates_resource_identity
  ON approval_gates(resource_namespace, resource_kind, resource_name, created_at DESC);
