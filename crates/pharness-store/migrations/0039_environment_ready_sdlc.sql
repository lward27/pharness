ALTER TABLE work_items ADD COLUMN environment_profile_id TEXT;
ALTER TABLE work_items ADD COLUMN run_budget_json TEXT NOT NULL DEFAULT '{"initial_turns":48,"hard_turns":100,"initial_tokens":400000,"hard_tokens":1000000,"active_execution_seconds":3600,"recoverable_tool_errors":4,"identical_failures":2,"verification_reserve_turns":8}';
ALTER TABLE work_items ADD COLUMN repository_contract_json TEXT;
ALTER TABLE work_items ADD COLUMN repository_contract_hash TEXT;
ALTER TABLE work_items ADD COLUMN environment_preparation_status TEXT NOT NULL DEFAULT 'not_required';
ALTER TABLE work_items ADD COLUMN current_environment_snapshot_id TEXT;

ALTER TABLE runs ADD COLUMN run_budget_json TEXT NOT NULL DEFAULT '{"initial_turns":48,"hard_turns":100,"initial_tokens":400000,"hard_tokens":1000000,"active_execution_seconds":3600,"recoverable_tool_errors":4,"identical_failures":2,"verification_reserve_turns":8}';
ALTER TABLE runs ADD COLUMN budget_consumption_json TEXT NOT NULL DEFAULT '{"allowed_turns":48,"allowed_tokens":400000,"turns_used":0,"tokens_used":0,"active_execution_seconds_used":0,"extensions":0}';
ALTER TABLE runs ADD COLUMN stop_reason TEXT;

CREATE TABLE environment_preparations (
  id TEXT PRIMARY KEY,
  work_item_id TEXT NOT NULL REFERENCES work_items(id),
  workspace_id TEXT NOT NULL REFERENCES workspaces(id),
  run_id TEXT REFERENCES runs(id),
  status TEXT NOT NULL,
  environment_profile_id TEXT NOT NULL,
  source_commit TEXT NOT NULL,
  project_contract_json TEXT,
  project_contract_hash TEXT,
  environment_snapshot_json TEXT,
  logs_json TEXT NOT NULL DEFAULT '[]',
  error TEXT,
  started_at TEXT,
  finished_at TEXT,
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL
);

CREATE UNIQUE INDEX idx_environment_preparations_workspace
  ON environment_preparations(workspace_id);
CREATE INDEX idx_environment_preparations_work_item
  ON environment_preparations(work_item_id, created_at DESC);
CREATE INDEX idx_environment_preparations_run
  ON environment_preparations(run_id);

CREATE TABLE budget_extensions (
  id TEXT PRIMARY KEY,
  work_item_id TEXT NOT NULL REFERENCES work_items(id),
  run_id TEXT NOT NULL REFERENCES runs(id),
  status TEXT NOT NULL,
  turn_increment INTEGER NOT NULL,
  token_increment INTEGER NOT NULL,
  state_hash TEXT NOT NULL,
  requested_at TEXT NOT NULL,
  approved_at TEXT,
  approved_by TEXT,
  approval_reason TEXT
);

CREATE INDEX idx_budget_extensions_run
  ON budget_extensions(run_id, requested_at DESC);
