CREATE TABLE subject_workspaces (
  id TEXT PRIMARY KEY,
  subject_kind TEXT NOT NULL,
  subject_id TEXT NOT NULL,
  run_id TEXT REFERENCES runs(id),
  status TEXT NOT NULL,
  source_repo TEXT NOT NULL,
  source_ref TEXT NOT NULL,
  source_commit TEXT NOT NULL,
  resolved_commit TEXT,
  branch TEXT,
  retention_status TEXT NOT NULL,
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL,
  status_changed_at TEXT NOT NULL,
  status_changed_by TEXT,
  status_reason TEXT
);

CREATE INDEX idx_subject_workspaces_subject
  ON subject_workspaces(subject_kind, subject_id, created_at DESC);
CREATE INDEX idx_subject_workspaces_run
  ON subject_workspaces(run_id, created_at DESC);

INSERT INTO subject_workspaces (
  id, subject_kind, subject_id, run_id, status, source_repo, source_ref,
  source_commit, resolved_commit, branch, retention_status, created_at,
  updated_at, status_changed_at, status_changed_by, status_reason
)
SELECT id, 'work_item', work_item_id, run_id, status, source_repo, source_ref,
       COALESCE(resolved_commit, ''), resolved_commit, branch, retention_status,
       created_at, updated_at, status_changed_at, status_changed_by, status_reason
FROM workspaces;

CREATE TABLE subject_environment_preparations (
  id TEXT PRIMARY KEY,
  subject_kind TEXT NOT NULL,
  subject_id TEXT NOT NULL,
  workspace_id TEXT NOT NULL REFERENCES subject_workspaces(id),
  run_id TEXT REFERENCES runs(id),
  status TEXT NOT NULL,
  environment_profile_id TEXT NOT NULL,
  source_commit TEXT NOT NULL,
  input_hash TEXT NOT NULL,
  input_json TEXT NOT NULL,
  repository_contract_json TEXT,
  repository_contract_hash TEXT,
  environment_snapshot_json TEXT,
  acceptance_results_json TEXT NOT NULL DEFAULT '[]',
  logs_json TEXT NOT NULL DEFAULT '[]',
  error_code TEXT,
  started_at TEXT,
  finished_at TEXT,
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL
);

CREATE UNIQUE INDEX idx_subject_environment_preparations_workspace
  ON subject_environment_preparations(workspace_id);
CREATE INDEX idx_subject_environment_preparations_subject
  ON subject_environment_preparations(subject_kind, subject_id, created_at DESC);
CREATE INDEX idx_subject_environment_preparations_run
  ON subject_environment_preparations(run_id);

INSERT INTO subject_environment_preparations (
  id, subject_kind, subject_id, workspace_id, run_id, status,
  environment_profile_id, source_commit, input_hash, input_json,
  repository_contract_json, repository_contract_hash,
  environment_snapshot_json, logs_json, error_code, started_at, finished_at,
  created_at, updated_at
)
SELECT id, 'work_item', work_item_id, workspace_id, run_id, status,
       environment_profile_id, source_commit,
       'legacy:' || id, json_object('legacy_environment_preparation_id', id),
       project_contract_json, project_contract_hash, environment_snapshot_json,
       logs_json, error, started_at, finished_at, created_at, updated_at
FROM environment_preparations;
