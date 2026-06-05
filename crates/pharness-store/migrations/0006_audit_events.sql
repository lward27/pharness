CREATE TABLE audit_events (
  id TEXT PRIMARY KEY,
  kind TEXT NOT NULL,
  actor TEXT,
  resource_kind TEXT NOT NULL,
  resource_id TEXT NOT NULL,
  run_id TEXT REFERENCES runs(id),
  payload_json TEXT NOT NULL,
  created_at TEXT NOT NULL
);

CREATE INDEX idx_audit_events_created
ON audit_events(created_at DESC);

CREATE INDEX idx_audit_events_resource
ON audit_events(resource_kind, resource_id, created_at DESC);

CREATE INDEX idx_audit_events_run
ON audit_events(run_id, created_at DESC);
