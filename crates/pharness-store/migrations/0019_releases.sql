CREATE TABLE releases (
  id TEXT PRIMARY KEY,
  deployment_intent_id TEXT NOT NULL REFERENCES deployment_intents(id),
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
  release_kind TEXT NOT NULL,
  target_environment TEXT,
  target_namespace TEXT,
  argo_application TEXT,
  version TEXT,
  commit_sha TEXT,
  image_digest TEXT,
  rollback_ref TEXT,
  release_json TEXT NOT NULL DEFAULT '{}',
  created_at TEXT NOT NULL,
  updated_at TEXT,
  status_changed_at TEXT,
  status_changed_by TEXT,
  status_reason TEXT
);

CREATE UNIQUE INDEX idx_releases_deployment_intent
  ON releases(deployment_intent_id);

CREATE INDEX idx_releases_pipeline_intent
  ON releases(pipeline_intent_id, created_at DESC);

CREATE INDEX idx_releases_change_set
  ON releases(change_set_id, created_at DESC);

CREATE INDEX idx_releases_work_plan
  ON releases(work_plan_id, created_at DESC);

CREATE INDEX idx_releases_status_created
  ON releases(status, created_at DESC);

CREATE INDEX idx_releases_run
  ON releases(run_id, created_at DESC);

CREATE INDEX idx_releases_remediation_plan
  ON releases(remediation_plan_id, created_at DESC);

CREATE INDEX idx_releases_incident
  ON releases(incident_id, created_at DESC);

CREATE INDEX idx_releases_target
  ON releases(target_environment, target_namespace, argo_application, created_at DESC);

CREATE INDEX idx_releases_artifacts
  ON releases(version, commit_sha, image_digest, created_at DESC);
