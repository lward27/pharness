CREATE TABLE controller_waits (
  id TEXT PRIMARY KEY,
  work_item_id TEXT NOT NULL REFERENCES work_items(id),
  session_id TEXT NOT NULL REFERENCES sessions(id),
  run_id TEXT REFERENCES runs(id),
  status TEXT NOT NULL,
  wait_kind TEXT NOT NULL,
  subject_kind TEXT NOT NULL,
  subject_id TEXT NOT NULL,
  next_check_at TEXT NOT NULL,
  deadline_at TEXT NOT NULL,
  max_checks INTEGER NOT NULL,
  check_count INTEGER NOT NULL DEFAULT 0,
  data_json TEXT NOT NULL DEFAULT '{}',
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL,
  resolved_at TEXT,
  resolution_reason TEXT
);

CREATE UNIQUE INDEX idx_controller_waits_active_work_item
  ON controller_waits(work_item_id)
  WHERE status = 'active';

CREATE INDEX idx_controller_waits_due
  ON controller_waits(status, next_check_at ASC);

CREATE INDEX idx_controller_waits_work_item_created
  ON controller_waits(work_item_id, created_at DESC);
