ALTER TABLE work_plans
  ADD COLUMN updated_at TEXT;

ALTER TABLE work_plans
  ADD COLUMN revision INTEGER NOT NULL DEFAULT 1;

ALTER TABLE work_plans
  ADD COLUMN status_changed_at TEXT;

ALTER TABLE work_plans
  ADD COLUMN status_changed_by TEXT;

ALTER TABLE work_plans
  ADD COLUMN status_reason TEXT;

ALTER TABLE approval_gates
  ADD COLUMN stale_at TEXT;

ALTER TABLE approval_gates
  ADD COLUMN stale_by TEXT;

ALTER TABLE approval_gates
  ADD COLUMN stale_reason TEXT;

CREATE INDEX idx_approval_gates_stale_created
  ON approval_gates(status, stale_at DESC);
