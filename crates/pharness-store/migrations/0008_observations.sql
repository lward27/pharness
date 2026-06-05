CREATE TABLE observations (
  id TEXT PRIMARY KEY,
  session_id TEXT NOT NULL REFERENCES sessions(id),
  run_id TEXT REFERENCES runs(id),
  source TEXT NOT NULL,
  kind TEXT NOT NULL,
  subject TEXT NOT NULL,
  summary TEXT NOT NULL,
  resource_ref_json TEXT,
  artifact_id TEXT REFERENCES artifacts(id),
  data_json TEXT NOT NULL DEFAULT '{}',
  observed_at TEXT NOT NULL
);

CREATE INDEX idx_observations_run ON observations(run_id, observed_at DESC);
CREATE INDEX idx_observations_source_kind ON observations(source, kind, observed_at DESC);
