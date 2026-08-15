-- Durable provenance lets the operator console separate human/controller work
-- from smoke fixtures without relying on title conventions or transient UI state.
ALTER TABLE work_items
  ADD COLUMN origin TEXT NOT NULL DEFAULT 'legacy'
  CHECK (origin IN ('operator', 'controller', 'worker', 'smoke', 'system', 'legacy'));

ALTER TABLE runs
  ADD COLUMN origin TEXT NOT NULL DEFAULT 'legacy'
  CHECK (origin IN ('operator', 'controller', 'worker', 'smoke', 'system', 'legacy'));

ALTER TABLE approvals
  ADD COLUMN origin TEXT NOT NULL DEFAULT 'legacy'
  CHECK (origin IN ('operator', 'controller', 'worker', 'smoke', 'system', 'legacy'));

ALTER TABLE approval_gates
  ADD COLUMN origin TEXT NOT NULL DEFAULT 'legacy'
  CHECK (origin IN ('operator', 'controller', 'worker', 'smoke', 'system', 'legacy'));

CREATE INDEX idx_work_items_origin_created
  ON work_items(origin, created_at DESC);

CREATE INDEX idx_runs_origin_started
  ON runs(origin, started_at DESC);

CREATE INDEX idx_approvals_origin_requested
  ON approvals(origin, requested_at DESC);

CREATE INDEX idx_approval_gates_origin_created
  ON approval_gates(origin, created_at DESC);
