CREATE TABLE pipeline_intents (
  id TEXT PRIMARY KEY,
  change_set_id TEXT NOT NULL REFERENCES change_sets(id),
  work_plan_id TEXT NOT NULL REFERENCES work_plans(id),
  remediation_plan_id TEXT NOT NULL REFERENCES remediation_plans(id),
  incident_id TEXT NOT NULL REFERENCES incidents(id),
  session_id TEXT NOT NULL REFERENCES sessions(id),
  run_id TEXT REFERENCES runs(id),
  status TEXT NOT NULL,
  title TEXT NOT NULL,
  summary TEXT NOT NULL,
  risk_level TEXT NOT NULL,
  intent_kind TEXT NOT NULL,
  resource_namespace TEXT,
  resource_kind TEXT,
  resource_name TEXT,
  intent_json TEXT NOT NULL DEFAULT '{}',
  created_at TEXT NOT NULL,
  updated_at TEXT,
  status_changed_at TEXT,
  status_changed_by TEXT,
  status_reason TEXT
);

CREATE UNIQUE INDEX idx_pipeline_intents_change_set
  ON pipeline_intents(change_set_id);

CREATE INDEX idx_pipeline_intents_work_plan
  ON pipeline_intents(work_plan_id, created_at DESC);

CREATE INDEX idx_pipeline_intents_status_created
  ON pipeline_intents(status, created_at DESC);

CREATE INDEX idx_pipeline_intents_run
  ON pipeline_intents(run_id, created_at DESC);

CREATE INDEX idx_pipeline_intents_remediation_plan
  ON pipeline_intents(remediation_plan_id, created_at DESC);

CREATE INDEX idx_pipeline_intents_incident
  ON pipeline_intents(incident_id, created_at DESC);

CREATE INDEX idx_pipeline_intents_resource_identity
  ON pipeline_intents(resource_namespace, resource_kind, resource_name, created_at DESC);
