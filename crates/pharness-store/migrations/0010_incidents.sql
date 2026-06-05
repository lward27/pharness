CREATE TABLE incidents (
  id TEXT PRIMARY KEY,
  observation_id TEXT NOT NULL REFERENCES observations(id),
  session_id TEXT NOT NULL REFERENCES sessions(id),
  run_id TEXT REFERENCES runs(id),
  status TEXT NOT NULL,
  severity TEXT NOT NULL,
  title TEXT NOT NULL,
  summary TEXT NOT NULL,
  resource_namespace TEXT,
  resource_kind TEXT,
  resource_name TEXT,
  data_json TEXT NOT NULL DEFAULT '{}',
  created_at TEXT NOT NULL
);

CREATE INDEX idx_incidents_status_created
  ON incidents(status, created_at DESC);

CREATE INDEX idx_incidents_run
  ON incidents(run_id, created_at DESC);

CREATE INDEX idx_incidents_resource_identity
  ON incidents(resource_namespace, resource_kind, resource_name, created_at DESC);
