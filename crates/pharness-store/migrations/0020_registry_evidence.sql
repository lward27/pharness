CREATE TABLE registry_evidence (
  id TEXT PRIMARY KEY,
  release_id TEXT NOT NULL REFERENCES releases(id),
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
  registry TEXT,
  repository TEXT,
  image_ref TEXT,
  image_digest TEXT,
  tag TEXT,
  source TEXT NOT NULL,
  verification_status TEXT NOT NULL,
  evidence_json TEXT NOT NULL DEFAULT '{}',
  created_at TEXT NOT NULL,
  updated_at TEXT,
  status_changed_at TEXT,
  status_changed_by TEXT,
  status_reason TEXT
);

CREATE UNIQUE INDEX idx_registry_evidence_release
  ON registry_evidence(release_id);

CREATE INDEX idx_registry_evidence_deployment_intent
  ON registry_evidence(deployment_intent_id, created_at DESC);

CREATE INDEX idx_registry_evidence_pipeline_intent
  ON registry_evidence(pipeline_intent_id, created_at DESC);

CREATE INDEX idx_registry_evidence_change_set
  ON registry_evidence(change_set_id, created_at DESC);

CREATE INDEX idx_registry_evidence_work_plan
  ON registry_evidence(work_plan_id, created_at DESC);

CREATE INDEX idx_registry_evidence_status_created
  ON registry_evidence(status, created_at DESC);

CREATE INDEX idx_registry_evidence_run
  ON registry_evidence(run_id, created_at DESC);

CREATE INDEX idx_registry_evidence_release_status
  ON registry_evidence(release_id, status, created_at DESC);

CREATE INDEX idx_registry_evidence_artifact
  ON registry_evidence(registry, repository, image_digest, image_ref, tag, created_at DESC);
