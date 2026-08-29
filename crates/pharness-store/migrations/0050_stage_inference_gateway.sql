CREATE TABLE stage_inference_selections (
  id TEXT PRIMARY KEY,
  subject_kind TEXT NOT NULL,
  subject_id TEXT NOT NULL,
  stage_key TEXT NOT NULL,
  target_id TEXT NOT NULL,
  target_revision TEXT NOT NULL,
  target_hash TEXT NOT NULL,
  policy_id TEXT NOT NULL,
  policy_revision TEXT NOT NULL,
  policy_hash TEXT NOT NULL,
  effective_settings_json TEXT NOT NULL,
  resolved_binding_json TEXT NOT NULL,
  binding_hash TEXT NOT NULL,
  actor TEXT NOT NULL,
  reason TEXT NOT NULL,
  state_hash TEXT NOT NULL,
  supersedes_selection_id TEXT REFERENCES stage_inference_selections(id),
  stage_execution_id TEXT REFERENCES stage_executions(id),
  run_id TEXT REFERENCES runs(id),
  created_at TEXT NOT NULL
);

CREATE INDEX idx_stage_inference_selections_subject
  ON stage_inference_selections(subject_kind, subject_id, stage_key, created_at, id);
CREATE UNIQUE INDEX idx_stage_inference_selection_execution
  ON stage_inference_selections(stage_execution_id)
  WHERE stage_execution_id IS NOT NULL;
CREATE UNIQUE INDEX idx_stage_inference_selection_run
  ON stage_inference_selections(run_id)
  WHERE run_id IS NOT NULL;

CREATE TRIGGER stage_inference_selections_immutable_update
BEFORE UPDATE ON stage_inference_selections
BEGIN
  SELECT RAISE(ABORT, 'stage inference selections are immutable');
END;

CREATE TRIGGER stage_inference_selections_immutable_delete
BEFORE DELETE ON stage_inference_selections
BEGIN
  SELECT RAISE(ABORT, 'stage inference selections are immutable');
END;

CREATE TABLE inference_target_verifications (
  id TEXT PRIMARY KEY,
  target_id TEXT NOT NULL,
  target_revision TEXT NOT NULL,
  target_hash TEXT NOT NULL,
  status TEXT NOT NULL CHECK (status IN ('passed', 'failed', 'unavailable')),
  reachability TEXT NOT NULL,
  model_visible INTEGER NOT NULL,
  streaming_compatible INTEGER NOT NULL,
  tool_compatible INTEGER NOT NULL,
  observed_capabilities_json TEXT NOT NULL,
  sanitized_failure TEXT,
  actor TEXT NOT NULL,
  reason TEXT NOT NULL,
  config_hash TEXT NOT NULL,
  created_at TEXT NOT NULL,
  expires_at TEXT NOT NULL
);

CREATE INDEX idx_inference_target_verifications_lookup
  ON inference_target_verifications(target_id, target_revision, target_hash, created_at DESC);

CREATE TRIGGER inference_target_verifications_immutable_update
BEFORE UPDATE ON inference_target_verifications
BEGIN
  SELECT RAISE(ABORT, 'inference target verifications are immutable');
END;

CREATE TRIGGER inference_target_verifications_immutable_delete
BEFORE DELETE ON inference_target_verifications
BEGIN
  SELECT RAISE(ABORT, 'inference target verifications are immutable');
END;

CREATE TABLE inference_policy_qualifications (
  id TEXT PRIMARY KEY,
  policy_id TEXT NOT NULL,
  policy_revision TEXT NOT NULL,
  policy_hash TEXT NOT NULL,
  target_id TEXT NOT NULL,
  target_revision TEXT NOT NULL,
  target_hash TEXT NOT NULL,
  agent_profile_id TEXT NOT NULL,
  agent_profile_hash TEXT NOT NULL,
  suite_id TEXT NOT NULL,
  suite_hash TEXT NOT NULL,
  runtime_revision TEXT NOT NULL,
  attempts INTEGER NOT NULL,
  metrics_json TEXT NOT NULL,
  verdict TEXT NOT NULL CHECK (verdict IN ('passed', 'failed')),
  evidence_artifact_id TEXT,
  actor TEXT NOT NULL,
  reason TEXT NOT NULL,
  created_at TEXT NOT NULL
);

CREATE INDEX idx_inference_policy_qualifications_lookup
  ON inference_policy_qualifications(policy_id, policy_revision, policy_hash, created_at DESC);

CREATE TRIGGER inference_policy_qualifications_immutable_update
BEFORE UPDATE ON inference_policy_qualifications
BEGIN
  SELECT RAISE(ABORT, 'inference policy qualifications are immutable');
END;

CREATE TRIGGER inference_policy_qualifications_immutable_delete
BEFORE DELETE ON inference_policy_qualifications
BEGIN
  SELECT RAISE(ABORT, 'inference policy qualifications are immutable');
END;

CREATE TABLE model_grant_issuances (
  run_id TEXT NOT NULL REFERENCES runs(id),
  request_sequence INTEGER NOT NULL,
  selection_id TEXT NOT NULL REFERENCES stage_inference_selections(id),
  request_body_hash TEXT NOT NULL,
  nonce TEXT NOT NULL UNIQUE,
  issued_at_epoch_seconds INTEGER NOT NULL,
  expires_at_epoch_seconds INTEGER NOT NULL,
  PRIMARY KEY (run_id, request_sequence)
);

CREATE TRIGGER model_grant_issuances_immutable_update
BEFORE UPDATE ON model_grant_issuances
BEGIN
  SELECT RAISE(ABORT, 'model grant issuances are immutable');
END;

CREATE TRIGGER model_grant_issuances_immutable_delete
BEFORE DELETE ON model_grant_issuances
BEGIN
  SELECT RAISE(ABORT, 'model grant issuances are immutable');
END;

CREATE TABLE inference_evaluations (
  id TEXT PRIMARY KEY,
  status TEXT NOT NULL CHECK (status IN ('queued', 'running', 'completed', 'failed')),
  suite_id TEXT NOT NULL,
  suite_hash TEXT NOT NULL,
  attempts INTEGER NOT NULL,
  agent_profile_id TEXT NOT NULL,
  agent_profile_hash TEXT NOT NULL,
  target_id TEXT NOT NULL,
  target_revision TEXT NOT NULL,
  target_hash TEXT NOT NULL,
  policy_id TEXT NOT NULL,
  policy_revision TEXT NOT NULL,
  policy_hash TEXT NOT NULL,
  resolved_binding_json TEXT NOT NULL,
  binding_hash TEXT NOT NULL,
  runtime_revision TEXT NOT NULL,
  actor TEXT NOT NULL,
  reason TEXT NOT NULL,
  config_hash TEXT NOT NULL,
  job_name TEXT,
  report_json TEXT,
  report_hash TEXT,
  failure TEXT,
  qualification_id TEXT REFERENCES inference_policy_qualifications(id),
  created_at TEXT NOT NULL,
  started_at TEXT,
  finished_at TEXT
);

CREATE INDEX idx_inference_evaluations_policy
  ON inference_evaluations(policy_id, policy_revision, created_at DESC, id DESC);

CREATE TABLE inference_evaluation_grant_issuances (
  evaluation_id TEXT NOT NULL REFERENCES inference_evaluations(id),
  fixture_run_id TEXT NOT NULL,
  request_sequence INTEGER NOT NULL,
  request_body_hash TEXT NOT NULL,
  nonce TEXT NOT NULL UNIQUE,
  issued_at_epoch_seconds INTEGER NOT NULL,
  expires_at_epoch_seconds INTEGER NOT NULL,
  PRIMARY KEY (evaluation_id, fixture_run_id, request_sequence)
);

CREATE TRIGGER inference_evaluation_grants_immutable_update
BEFORE UPDATE ON inference_evaluation_grant_issuances
BEGIN
  SELECT RAISE(ABORT, 'inference evaluation model grant issuances are immutable');
END;

CREATE TRIGGER inference_evaluation_grants_immutable_delete
BEFORE DELETE ON inference_evaluation_grant_issuances
BEGIN
  SELECT RAISE(ABORT, 'inference evaluation model grant issuances are immutable');
END;
