ALTER TABLE approval_gates
  ADD COLUMN decided_at TEXT;

ALTER TABLE approval_gates
  ADD COLUMN decided_by TEXT;

ALTER TABLE approval_gates
  ADD COLUMN decision_reason TEXT;
