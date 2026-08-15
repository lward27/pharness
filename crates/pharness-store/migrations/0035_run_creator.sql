-- Operator filters must be backed by a durable identity, not inferred from an origin label.
ALTER TABLE runs
  ADD COLUMN created_by TEXT;

CREATE INDEX idx_runs_created_by_started
  ON runs(created_by, started_at DESC);
