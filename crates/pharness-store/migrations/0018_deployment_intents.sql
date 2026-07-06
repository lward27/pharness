CREATE TABLE deployment_intents (
  id TEXT PRIMARY KEY,
  pipeline_intent_id TEXT NOT NULL REFERENCES pipeline_intents(id),
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
  target_environment TEXT,
  target_namespace TEXT,
  argo_application TEXT,
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

CREATE UNIQUE INDEX idx_deployment_intents_pipeline_intent
  ON deployment_intents(pipeline_intent_id);

CREATE INDEX idx_deployment_intents_change_set
  ON deployment_intents(change_set_id, created_at DESC);

CREATE INDEX idx_deployment_intents_work_plan
  ON deployment_intents(work_plan_id, created_at DESC);

CREATE INDEX idx_deployment_intents_status_created
  ON deployment_intents(status, created_at DESC);

CREATE INDEX idx_deployment_intents_run
  ON deployment_intents(run_id, created_at DESC);

CREATE INDEX idx_deployment_intents_remediation_plan
  ON deployment_intents(remediation_plan_id, created_at DESC);

CREATE INDEX idx_deployment_intents_incident
  ON deployment_intents(incident_id, created_at DESC);

CREATE INDEX idx_deployment_intents_target
  ON deployment_intents(target_environment, target_namespace, argo_application, created_at DESC);

CREATE INDEX idx_deployment_intents_resource_identity
  ON deployment_intents(resource_namespace, resource_kind, resource_name, created_at DESC);
