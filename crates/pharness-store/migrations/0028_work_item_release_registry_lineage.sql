-- WorkItem delivery reaches Release and RegistryEvidence through its durable
-- DeploymentIntent, PipelineIntent, ChangeSet, and WorkPlan lineage. These
-- records must not require invented incident records.
PRAGMA defer_foreign_keys = ON;

CREATE TABLE releases_v2 (
  id TEXT PRIMARY KEY,
  deployment_intent_id TEXT NOT NULL REFERENCES deployment_intents(id),
  pipeline_intent_id TEXT NOT NULL REFERENCES pipeline_intents(id),
  change_set_id TEXT NOT NULL REFERENCES change_sets(id),
  work_plan_id TEXT NOT NULL REFERENCES work_plans(id),
  remediation_plan_id TEXT REFERENCES remediation_plans(id),
  incident_id TEXT REFERENCES incidents(id),
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
INSERT INTO releases_v2 SELECT * FROM releases;
DROP TABLE releases;
ALTER TABLE releases_v2 RENAME TO releases;

CREATE TABLE registry_evidence_v2 (
  id TEXT PRIMARY KEY,
  release_id TEXT NOT NULL REFERENCES releases(id),
  deployment_intent_id TEXT NOT NULL REFERENCES deployment_intents(id),
  pipeline_intent_id TEXT NOT NULL REFERENCES pipeline_intents(id),
  change_set_id TEXT NOT NULL REFERENCES change_sets(id),
  work_plan_id TEXT NOT NULL REFERENCES work_plans(id),
  remediation_plan_id TEXT REFERENCES remediation_plans(id),
  incident_id TEXT REFERENCES incidents(id),
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
INSERT INTO registry_evidence_v2 SELECT * FROM registry_evidence;
DROP TABLE registry_evidence;
ALTER TABLE registry_evidence_v2 RENAME TO registry_evidence;

CREATE UNIQUE INDEX idx_releases_deployment_intent ON releases(deployment_intent_id);
CREATE INDEX idx_releases_pipeline_intent ON releases(pipeline_intent_id, created_at DESC);
CREATE INDEX idx_releases_change_set ON releases(change_set_id, created_at DESC);
CREATE INDEX idx_releases_work_plan ON releases(work_plan_id, created_at DESC);
CREATE INDEX idx_releases_status_created ON releases(status, created_at DESC);
CREATE INDEX idx_releases_run ON releases(run_id, created_at DESC);
CREATE INDEX idx_releases_remediation_plan ON releases(remediation_plan_id, created_at DESC);
CREATE INDEX idx_releases_incident ON releases(incident_id, created_at DESC);
CREATE INDEX idx_releases_target ON releases(target_environment, target_namespace, argo_application, created_at DESC);
CREATE INDEX idx_releases_artifacts ON releases(version, commit_sha, image_digest, created_at DESC);

CREATE UNIQUE INDEX idx_registry_evidence_release ON registry_evidence(release_id);
CREATE INDEX idx_registry_evidence_deployment_intent ON registry_evidence(deployment_intent_id, created_at DESC);
CREATE INDEX idx_registry_evidence_pipeline_intent ON registry_evidence(pipeline_intent_id, created_at DESC);
CREATE INDEX idx_registry_evidence_change_set ON registry_evidence(change_set_id, created_at DESC);
CREATE INDEX idx_registry_evidence_work_plan ON registry_evidence(work_plan_id, created_at DESC);
CREATE INDEX idx_registry_evidence_status_created ON registry_evidence(status, created_at DESC);
CREATE INDEX idx_registry_evidence_run ON registry_evidence(run_id, created_at DESC);
CREATE INDEX idx_registry_evidence_release_status ON registry_evidence(release_id, status, created_at DESC);
CREATE INDEX idx_registry_evidence_artifact ON registry_evidence(registry, repository, image_digest, image_ref, tag, created_at DESC);
