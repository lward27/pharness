CREATE TABLE operator_annotation_decisions (
  id TEXT PRIMARY KEY,
  annotation_id TEXT NOT NULL UNIQUE REFERENCES operator_annotations(id),
  work_item_id TEXT NOT NULL REFERENCES work_items(id),
  decision TEXT NOT NULL,
  action_id TEXT NOT NULL,
  actor TEXT NOT NULL,
  reason TEXT NOT NULL,
  state_hash TEXT NOT NULL,
  created_at TEXT NOT NULL,
  CHECK (decision IN ('context_recorded', 'stage_repeat_started', 'replan_started', 'declined'))
);

CREATE INDEX idx_operator_annotation_decisions_work_item
  ON operator_annotation_decisions(work_item_id, created_at, id);

CREATE TRIGGER prevent_operator_annotation_update
BEFORE UPDATE ON operator_annotations
BEGIN
  SELECT RAISE(ABORT, 'operator annotations are append-only');
END;

CREATE TRIGGER prevent_operator_annotation_delete
BEFORE DELETE ON operator_annotations
BEGIN
  SELECT RAISE(ABORT, 'operator annotations are append-only');
END;

CREATE TRIGGER prevent_operator_annotation_decision_update
BEFORE UPDATE ON operator_annotation_decisions
BEGIN
  SELECT RAISE(ABORT, 'operator annotation decisions are immutable');
END;

CREATE TRIGGER prevent_operator_annotation_decision_delete
BEFORE DELETE ON operator_annotation_decisions
BEGIN
  SELECT RAISE(ABORT, 'operator annotation decisions are immutable');
END;
