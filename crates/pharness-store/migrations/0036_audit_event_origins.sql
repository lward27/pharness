-- Audit filters must operate on durable provenance, not UI-derived labels.
ALTER TABLE audit_events
  ADD COLUMN origin TEXT NOT NULL DEFAULT 'legacy'
  CHECK (origin IN ('operator', 'controller', 'worker', 'smoke', 'system', 'legacy'));

UPDATE audit_events
SET origin = COALESCE(
  (SELECT origin FROM runs WHERE id = audit_events.run_id),
  CASE audit_events.resource_kind
    WHEN 'run' THEN (SELECT origin FROM runs WHERE id = audit_events.resource_id)
    WHEN 'work_item' THEN (SELECT origin FROM work_items WHERE id = audit_events.resource_id)
    WHEN 'approval' THEN (SELECT origin FROM approvals WHERE id = audit_events.resource_id)
    WHEN 'approval_gate' THEN (SELECT origin FROM approval_gates WHERE id = audit_events.resource_id)
  END,
  'legacy'
);

CREATE INDEX idx_audit_events_origin_created
  ON audit_events(origin, created_at DESC);
