CREATE TABLE agent_execution_selections (
  id TEXT PRIMARY KEY,
  subject_kind TEXT NOT NULL,
  subject_id TEXT NOT NULL,
  stage_key TEXT NOT NULL,
  policy_id TEXT NOT NULL,
  policy_revision TEXT NOT NULL,
  policy_hash TEXT NOT NULL,
  resolved_binding_json TEXT NOT NULL,
  binding_hash TEXT NOT NULL,
  actor TEXT NOT NULL,
  reason TEXT NOT NULL,
  state_hash TEXT NOT NULL,
  supersedes_selection_id TEXT REFERENCES agent_execution_selections(id),
  stage_execution_id TEXT REFERENCES stage_executions(id),
  run_id TEXT REFERENCES runs(id),
  created_at TEXT NOT NULL
);

CREATE INDEX idx_agent_execution_selections_subject
  ON agent_execution_selections(subject_kind, subject_id, stage_key, created_at, id);
CREATE UNIQUE INDEX idx_agent_execution_selection_execution
  ON agent_execution_selections(stage_execution_id)
  WHERE stage_execution_id IS NOT NULL;
CREATE UNIQUE INDEX idx_agent_execution_selection_run
  ON agent_execution_selections(run_id)
  WHERE run_id IS NOT NULL;

CREATE TRIGGER agent_execution_selections_immutable_update
BEFORE UPDATE ON agent_execution_selections
BEGIN
  SELECT RAISE(ABORT, 'agent execution selections are immutable');
END;

CREATE TRIGGER agent_execution_selections_immutable_delete
BEFORE DELETE ON agent_execution_selections
BEGIN
  SELECT RAISE(ABORT, 'agent execution selections are immutable');
END;

CREATE TABLE agent_execution_policy_qualifications (
  id TEXT PRIMARY KEY,
  policy_id TEXT NOT NULL,
  policy_revision TEXT NOT NULL,
  policy_hash TEXT NOT NULL,
  runtime_revision TEXT NOT NULL,
  suite_id TEXT NOT NULL,
  suite_hash TEXT NOT NULL,
  attempts INTEGER NOT NULL,
  metrics_json TEXT NOT NULL,
  verdict TEXT NOT NULL CHECK (verdict IN ('passed', 'failed', 'incomplete')),
  evidence_artifact_id TEXT REFERENCES artifacts(id),
  actor TEXT NOT NULL,
  reason TEXT NOT NULL,
  created_at TEXT NOT NULL
);

CREATE INDEX idx_agent_execution_policy_qualifications_lookup
  ON agent_execution_policy_qualifications(policy_id, policy_revision, created_at DESC, id DESC);

CREATE TRIGGER agent_execution_policy_qualifications_immutable_update
BEFORE UPDATE ON agent_execution_policy_qualifications
BEGIN
  SELECT RAISE(ABORT, 'agent execution policy qualifications are immutable');
END;

CREATE TRIGGER agent_execution_policy_qualifications_immutable_delete
BEFORE DELETE ON agent_execution_policy_qualifications
BEGIN
  SELECT RAISE(ABORT, 'agent execution policy qualifications are immutable');
END;

CREATE TABLE agent_host_enrollments (
  id TEXT PRIMARY KEY,
  display_name TEXT NOT NULL,
  host_pool TEXT NOT NULL,
  token_hash TEXT NOT NULL UNIQUE,
  actor TEXT NOT NULL,
  reason TEXT NOT NULL,
  created_at TEXT NOT NULL,
  expires_at TEXT NOT NULL,
  consumed_at TEXT,
  consumed_by_host_id TEXT
);

CREATE INDEX idx_agent_host_enrollments_expiry
  ON agent_host_enrollments(expires_at, consumed_at);

CREATE TABLE agent_hosts (
  id TEXT PRIMARY KEY,
  display_name TEXT NOT NULL,
  host_pool TEXT NOT NULL,
  lifecycle_state TEXT NOT NULL CHECK (
    lifecycle_state IN ('ready', 'draining', 'unavailable', 'retired')
  ),
  credential_hash TEXT NOT NULL,
  enrollment_id TEXT NOT NULL REFERENCES agent_host_enrollments(id),
  platform TEXT NOT NULL,
  architecture TEXT NOT NULL,
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL,
  last_contact_at TEXT,
  retired_at TEXT
);

CREATE INDEX idx_agent_hosts_pool_state
  ON agent_hosts(host_pool, lifecycle_state, updated_at);

CREATE TABLE agent_host_capability_snapshots (
  id TEXT PRIMARY KEY,
  host_id TEXT NOT NULL REFERENCES agent_hosts(id),
  platform TEXT NOT NULL,
  architecture TEXT NOT NULL,
  codex_version TEXT NOT NULL,
  podman_version TEXT,
  execution_mode TEXT NOT NULL CHECK (execution_mode IN ('standalone', 'kubernetes')),
  authentication_class TEXT NOT NULL CHECK (
    authentication_class IN ('chatgpt_session', 'api_key', 'workload_identity')
  ),
  authentication_ready INTEGER NOT NULL,
  supported_profiles_json TEXT NOT NULL,
  runner_images_json TEXT NOT NULL,
  available_slots INTEGER NOT NULL,
  storage_json TEXT NOT NULL,
  status TEXT NOT NULL CHECK (status IN ('passed', 'failed', 'unavailable')),
  blockers_json TEXT NOT NULL,
  content_hash TEXT NOT NULL,
  created_at TEXT NOT NULL,
  expires_at TEXT NOT NULL
);

CREATE INDEX idx_agent_host_capability_snapshots_lookup
  ON agent_host_capability_snapshots(host_id, created_at DESC, id DESC);

CREATE TRIGGER agent_host_capability_snapshots_immutable_update
BEFORE UPDATE ON agent_host_capability_snapshots
BEGIN
  SELECT RAISE(ABORT, 'agent host capability snapshots are immutable');
END;

CREATE TRIGGER agent_host_capability_snapshots_immutable_delete
BEFORE DELETE ON agent_host_capability_snapshots
BEGIN
  SELECT RAISE(ABORT, 'agent host capability snapshots are immutable');
END;

CREATE TABLE agent_leases (
  id TEXT PRIMARY KEY,
  run_id TEXT NOT NULL REFERENCES runs(id),
  stage_execution_id TEXT NOT NULL REFERENCES stage_executions(id),
  host_pool TEXT NOT NULL,
  pinned_host_id TEXT REFERENCES agent_hosts(id),
  host_id TEXT REFERENCES agent_hosts(id),
  workspace_id TEXT NOT NULL,
  environment_profile_id TEXT NOT NULL,
  runner_image TEXT NOT NULL,
  binding_hash TEXT NOT NULL,
  state TEXT NOT NULL CHECK (
    state IN ('queued', 'claimed', 'running', 'paused', 'completed', 'cancelled', 'abandoned', 'failed')
  ),
  lease_token_hash TEXT,
  remote_thread_id TEXT,
  completion_hash TEXT,
  error TEXT,
  created_at TEXT NOT NULL,
  claimed_at TEXT,
  heartbeat_at TEXT,
  expires_at TEXT,
  completed_at TEXT
);

CREATE UNIQUE INDEX idx_agent_leases_stage_execution
  ON agent_leases(stage_execution_id);
CREATE UNIQUE INDEX idx_agent_leases_run
  ON agent_leases(run_id);
CREATE INDEX idx_agent_leases_claim
  ON agent_leases(state, host_pool, pinned_host_id, environment_profile_id, created_at, id);
CREATE INDEX idx_agent_leases_host
  ON agent_leases(host_id, state, heartbeat_at);

CREATE TRIGGER agent_host_enrollment_consumer_exists
BEFORE UPDATE OF consumed_by_host_id ON agent_host_enrollments
WHEN NEW.consumed_by_host_id IS NOT NULL
 AND NOT EXISTS (SELECT 1 FROM agent_hosts WHERE id = NEW.consumed_by_host_id)
BEGIN
  SELECT RAISE(ABORT, 'agent host enrollment consumer does not exist');
END;
