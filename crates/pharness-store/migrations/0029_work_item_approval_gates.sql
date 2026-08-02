-- Approval gates have one durable origin: an incident remediation chain or a
-- WorkItem delivery chain. A null incident is not itself a WorkItem gate.
PRAGMA defer_foreign_keys = ON;

CREATE TABLE approval_gates_v2 (
  id TEXT PRIMARY KEY,
  work_item_id TEXT REFERENCES work_items(id),
  remediation_plan_id TEXT REFERENCES remediation_plans(id),
  incident_id TEXT REFERENCES incidents(id),
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
  created_at TEXT NOT NULL,
  decided_at TEXT,
  decided_by TEXT,
  decision_reason TEXT,
  stale_at TEXT,
  stale_by TEXT,
  stale_reason TEXT,
  CHECK (
    (work_item_id IS NOT NULL AND remediation_plan_id IS NULL AND incident_id IS NULL)
    OR (work_item_id IS NULL AND remediation_plan_id IS NOT NULL AND incident_id IS NOT NULL)
  )
);

INSERT INTO approval_gates_v2 (
  id, remediation_plan_id, incident_id, session_id, run_id, status, gate_kind,
  gate_order, title, summary, risk_level, resource_namespace, resource_kind,
  resource_name, gate_json, created_at, decided_at, decided_by, decision_reason,
  stale_at, stale_by, stale_reason
)
SELECT id, remediation_plan_id, incident_id, session_id, run_id, status, gate_kind,
       gate_order, title, summary, risk_level, resource_namespace, resource_kind,
       resource_name, gate_json, created_at, decided_at, decided_by, decision_reason,
       stale_at, stale_by, stale_reason
FROM approval_gates;
DROP TABLE approval_gates;
ALTER TABLE approval_gates_v2 RENAME TO approval_gates;

CREATE INDEX idx_approval_gates_work_item ON approval_gates(work_item_id, gate_order ASC);
CREATE INDEX idx_approval_gates_plan ON approval_gates(remediation_plan_id, gate_order ASC);
CREATE INDEX idx_approval_gates_status_created ON approval_gates(status, created_at DESC);
CREATE INDEX idx_approval_gates_run ON approval_gates(run_id, created_at DESC);
CREATE INDEX idx_approval_gates_resource_identity ON approval_gates(resource_namespace, resource_kind, resource_name, created_at DESC);
