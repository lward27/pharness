DROP INDEX IF EXISTS idx_environment_preparations_workspace;
CREATE INDEX idx_environment_preparations_workspace
  ON environment_preparations(workspace_id, created_at DESC);

DROP INDEX IF EXISTS idx_subject_environment_preparations_workspace;
CREATE INDEX idx_subject_environment_preparations_workspace
  ON subject_environment_preparations(workspace_id, created_at DESC);
