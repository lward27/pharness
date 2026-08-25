CREATE TABLE stage_executions (
  id TEXT PRIMARY KEY,
  work_item_id TEXT NOT NULL REFERENCES work_items(id),
  stage_key TEXT NOT NULL,
  sequence INTEGER NOT NULL,
  status TEXT NOT NULL,
  agent_profile_id TEXT,
  agent_profile_version TEXT,
  agent_profile_hash TEXT,
  context_pack_id TEXT,
  run_id TEXT REFERENCES runs(id),
  workspace_id TEXT REFERENCES workspaces(id),
  input_snapshot_json TEXT NOT NULL,
  input_hash TEXT NOT NULL,
  stop_reason TEXT,
  created_at TEXT NOT NULL,
  started_at TEXT,
  finished_at TEXT,
  UNIQUE (work_item_id, stage_key, sequence)
);

CREATE INDEX idx_stage_executions_work_item
  ON stage_executions(work_item_id, stage_key, sequence DESC);

CREATE TABLE stage_outcomes (
  id TEXT PRIMARY KEY,
  stage_execution_id TEXT NOT NULL UNIQUE REFERENCES stage_executions(id),
  work_item_id TEXT NOT NULL REFERENCES work_items(id),
  stage_key TEXT NOT NULL,
  status TEXT NOT NULL,
  schema_version TEXT NOT NULL,
  outcome_json TEXT NOT NULL,
  content_hash TEXT NOT NULL,
  state_version INTEGER NOT NULL,
  supersedes_outcome_id TEXT REFERENCES stage_outcomes(id),
  sealed_by TEXT NOT NULL,
  sealed_at TEXT NOT NULL
);

CREATE TRIGGER stage_outcomes_immutable_update
BEFORE UPDATE ON stage_outcomes
BEGIN
  SELECT RAISE(ABORT, 'stage outcomes are immutable');
END;

CREATE TRIGGER stage_outcomes_immutable_delete
BEFORE DELETE ON stage_outcomes
BEGIN
  SELECT RAISE(ABORT, 'stage outcomes are immutable');
END;

CREATE TABLE effective_stage_outcomes (
  work_item_id TEXT NOT NULL REFERENCES work_items(id),
  stage_key TEXT NOT NULL,
  outcome_id TEXT NOT NULL REFERENCES stage_outcomes(id),
  state_version INTEGER NOT NULL,
  changed_by TEXT NOT NULL,
  change_reason TEXT NOT NULL,
  changed_at TEXT NOT NULL,
  PRIMARY KEY (work_item_id, stage_key)
);

CREATE TABLE evidence_validations (
  id TEXT PRIMARY KEY,
  work_item_id TEXT NOT NULL REFERENCES work_items(id),
  stage_execution_id TEXT REFERENCES stage_executions(id),
  validator_key TEXT NOT NULL,
  schema_version TEXT NOT NULL,
  status TEXT NOT NULL,
  subject_json TEXT NOT NULL,
  evidence_refs_json TEXT NOT NULL,
  facts_json TEXT NOT NULL,
  contradictions_json TEXT NOT NULL,
  content_hash TEXT NOT NULL,
  validated_at TEXT NOT NULL
);

CREATE INDEX idx_evidence_validations_stage
  ON evidence_validations(stage_execution_id, validator_key, validated_at);

CREATE TABLE agent_context_packs (
  id TEXT PRIMARY KEY,
  work_item_id TEXT NOT NULL REFERENCES work_items(id),
  stage_execution_id TEXT NOT NULL UNIQUE REFERENCES stage_executions(id),
  schema_version TEXT NOT NULL,
  context_json TEXT NOT NULL,
  estimated_tokens INTEGER NOT NULL,
  content_hash TEXT NOT NULL,
  created_at TEXT NOT NULL
);

CREATE TABLE evidence_retrievals (
  id TEXT PRIMARY KEY,
  work_item_id TEXT NOT NULL REFERENCES work_items(id),
  stage_execution_id TEXT NOT NULL REFERENCES stage_executions(id),
  run_id TEXT NOT NULL REFERENCES runs(id),
  actor TEXT NOT NULL,
  evidence_kind TEXT NOT NULL,
  evidence_id TEXT NOT NULL,
  evidence_version TEXT NOT NULL,
  returned_hash TEXT NOT NULL,
  created_at TEXT NOT NULL
);

CREATE TABLE operator_annotations (
  id TEXT PRIMARY KEY,
  work_item_id TEXT NOT NULL REFERENCES work_items(id),
  target_kind TEXT NOT NULL,
  target_id TEXT NOT NULL,
  statement TEXT NOT NULL,
  evidence_refs_json TEXT NOT NULL,
  requested_effect TEXT NOT NULL,
  actor TEXT NOT NULL,
  reason TEXT NOT NULL,
  state_hash TEXT NOT NULL,
  created_at TEXT NOT NULL
);

CREATE INDEX idx_operator_annotations_work_item
  ON operator_annotations(work_item_id, created_at, id);

CREATE TABLE stage_chain_authorizations (
  id TEXT PRIMARY KEY,
  work_item_id TEXT NOT NULL REFERENCES work_items(id),
  work_plan_id TEXT NOT NULL REFERENCES work_plans(id),
  work_plan_revision INTEGER NOT NULL,
  product_model_snapshot_id TEXT NOT NULL REFERENCES product_model_snapshots(id),
  product_model_snapshot_hash TEXT NOT NULL,
  repository_id TEXT NOT NULL REFERENCES repositories(id),
  source_commit TEXT NOT NULL,
  workspace_id TEXT NOT NULL REFERENCES workspaces(id),
  writable_paths_json TEXT NOT NULL,
  profile_chain_json TEXT NOT NULL,
  budget_chain_json TEXT NOT NULL,
  state_hash TEXT NOT NULL,
  status TEXT NOT NULL,
  created_by TEXT NOT NULL,
  creation_reason TEXT NOT NULL,
  created_at TEXT NOT NULL,
  expires_at TEXT NOT NULL,
  revoked_at TEXT,
  revocation_reason TEXT
);

CREATE TABLE provider_check_set_observations (
  id TEXT PRIMARY KEY,
  source_delivery_intent_id TEXT NOT NULL REFERENCES source_delivery_intents(id),
  phase TEXT NOT NULL,
  repository_id TEXT NOT NULL REFERENCES repositories(id),
  pull_request_number INTEGER NOT NULL,
  head_sha TEXT NOT NULL,
  required_set_hash TEXT NOT NULL,
  authoritative_rules_succeeded INTEGER NOT NULL,
  status TEXT NOT NULL,
  required_checks_json TEXT NOT NULL,
  check_runs_json TEXT NOT NULL,
  commit_statuses_json TEXT NOT NULL,
  content_hash TEXT NOT NULL,
  observed_at TEXT NOT NULL,
  expires_at TEXT NOT NULL
);

CREATE INDEX idx_provider_checks_delivery
  ON provider_check_set_observations(source_delivery_intent_id, phase, observed_at DESC);
