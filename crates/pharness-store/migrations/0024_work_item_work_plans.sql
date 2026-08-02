-- no-transaction
PRAGMA foreign_keys = OFF;

CREATE TABLE work_plans_v2 (
  id TEXT PRIMARY KEY,
  work_item_id TEXT REFERENCES work_items(id),
  remediation_plan_id TEXT REFERENCES remediation_plans(id),
  incident_id TEXT REFERENCES incidents(id),
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
  created_at TEXT NOT NULL,
  updated_at TEXT,
  revision INTEGER NOT NULL DEFAULT 1,
  status_changed_at TEXT,
  status_changed_by TEXT,
  status_reason TEXT,
  CHECK (
    (work_item_id IS NOT NULL AND remediation_plan_id IS NULL AND incident_id IS NULL)
    OR
    (work_item_id IS NULL AND remediation_plan_id IS NOT NULL AND incident_id IS NOT NULL)
  )
);

INSERT INTO work_plans_v2 (
  id, work_item_id, remediation_plan_id, incident_id, session_id, run_id, status, title,
  summary, risk_level, requires_approval, resource_namespace, resource_kind, resource_name,
  work_plan_json, created_at, updated_at, revision, status_changed_at, status_changed_by,
  status_reason
)
SELECT
  id, NULL, remediation_plan_id, incident_id, session_id, run_id, status, title,
  summary, risk_level, requires_approval, resource_namespace, resource_kind, resource_name,
  work_plan_json, created_at, updated_at, revision, status_changed_at, status_changed_by,
  status_reason
FROM work_plans;

DROP TABLE work_plans;
ALTER TABLE work_plans_v2 RENAME TO work_plans;

CREATE UNIQUE INDEX idx_work_plans_remediation_plan
  ON work_plans(remediation_plan_id)
  WHERE remediation_plan_id IS NOT NULL;

CREATE UNIQUE INDEX idx_work_plans_work_item
  ON work_plans(work_item_id)
  WHERE work_item_id IS NOT NULL;

CREATE INDEX idx_work_plans_incident
  ON work_plans(incident_id, created_at DESC);

CREATE INDEX idx_work_plans_status_created
  ON work_plans(status, created_at DESC);

CREATE INDEX idx_work_plans_run
  ON work_plans(run_id, created_at DESC);

CREATE INDEX idx_work_plans_resource_identity
  ON work_plans(resource_namespace, resource_kind, resource_name, created_at DESC);

PRAGMA foreign_keys = ON;
