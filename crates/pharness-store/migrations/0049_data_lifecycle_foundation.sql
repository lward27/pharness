CREATE TABLE database_generations (
  id TEXT PRIMARY KEY,
  created_at TEXT NOT NULL,
  initializing_revision TEXT NOT NULL,
  schema_version TEXT NOT NULL,
  purpose TEXT NOT NULL
);

CREATE TABLE repository_binding_revision_scopes (
  id TEXT PRIMARY KEY,
  binding_revision_id TEXT NOT NULL REFERENCES repository_binding_revisions(id),
  path_glob TEXT NOT NULL,
  role TEXT NOT NULL CHECK (role IN (
    'source', 'delivery', 'automation', 'product_integration', 'documentation'
  )),
  service_id TEXT REFERENCES services(id),
  created_at TEXT NOT NULL,
  UNIQUE (binding_revision_id, path_glob, role, service_id)
);

CREATE INDEX idx_repository_binding_scopes_revision
  ON repository_binding_revision_scopes(binding_revision_id, path_glob, role);

CREATE TABLE retention_holds (
  id TEXT PRIMARY KEY,
  subject_kind TEXT NOT NULL,
  subject_id TEXT NOT NULL,
  reason TEXT NOT NULL,
  actor TEXT NOT NULL,
  created_at TEXT NOT NULL,
  expires_at TEXT,
  released_at TEXT,
  released_by TEXT,
  release_reason TEXT,
  state_hash TEXT NOT NULL
);

CREATE UNIQUE INDEX idx_retention_holds_active_subject
  ON retention_holds(subject_kind, subject_id)
  WHERE released_at IS NULL;

CREATE TABLE retention_previews (
  id TEXT PRIMARY KEY,
  database_generation_id TEXT NOT NULL REFERENCES database_generations(id),
  policy_version TEXT NOT NULL,
  status TEXT NOT NULL,
  preview_json TEXT NOT NULL,
  content_hash TEXT NOT NULL,
  state_hash TEXT NOT NULL,
  actor TEXT NOT NULL,
  reason TEXT NOT NULL,
  created_at TEXT NOT NULL,
  expires_at TEXT NOT NULL,
  executed_at TEXT
);

CREATE TABLE retention_receipts (
  id TEXT PRIMARY KEY,
  preview_id TEXT NOT NULL UNIQUE REFERENCES retention_previews(id),
  database_generation_id TEXT NOT NULL REFERENCES database_generations(id),
  policy_version TEXT NOT NULL,
  status TEXT NOT NULL,
  receipt_json TEXT NOT NULL,
  content_hash TEXT NOT NULL,
  actor TEXT NOT NULL,
  reason TEXT NOT NULL,
  created_at TEXT NOT NULL
);

CREATE TRIGGER retention_receipts_immutable_update
BEFORE UPDATE ON retention_receipts
BEGIN
  SELECT RAISE(ABORT, 'retention receipts are immutable');
END;

CREATE TRIGGER retention_receipts_immutable_delete
BEFORE DELETE ON retention_receipts
BEGIN
  SELECT RAISE(ABORT, 'retention receipts are immutable');
END;

CREATE TABLE archive_records (
  id TEXT PRIMARY KEY,
  database_generation_id TEXT NOT NULL REFERENCES database_generations(id),
  archived_generation_id TEXT NOT NULL,
  database_claim TEXT NOT NULL,
  archive_claim TEXT NOT NULL,
  database_sha256 TEXT NOT NULL,
  manifest_sha256 TEXT NOT NULL,
  archive_json TEXT NOT NULL,
  status TEXT NOT NULL,
  created_at TEXT NOT NULL,
  deletion_eligible_at TEXT NOT NULL,
  deleted_at TEXT,
  deletion_receipt_id TEXT REFERENCES retention_receipts(id)
);

CREATE TABLE sealed_run_summaries (
  id TEXT PRIMARY KEY,
  run_id TEXT NOT NULL UNIQUE REFERENCES runs(id),
  work_item_id TEXT REFERENCES work_items(id),
  summary_json TEXT NOT NULL,
  content_hash TEXT NOT NULL,
  sealed_at TEXT NOT NULL,
  compacted_at TEXT
);

CREATE TABLE evidence_validation_references (
  id TEXT PRIMARY KEY,
  evidence_validation_id TEXT NOT NULL REFERENCES evidence_validations(id),
  reference_kind TEXT NOT NULL,
  reference_id TEXT NOT NULL,
  reference_hash TEXT NOT NULL,
  created_at TEXT NOT NULL,
  UNIQUE (evidence_validation_id, reference_kind, reference_id)
);

CREATE INDEX idx_evidence_validation_references_target
  ON evidence_validation_references(reference_kind, reference_id);

ALTER TABLE artifacts ADD COLUMN content_hash TEXT;
ALTER TABLE artifacts ADD COLUMN retention_class TEXT NOT NULL DEFAULT 'operational';
ALTER TABLE artifacts ADD COLUMN purged_at TEXT;
ALTER TABLE file_changes ADD COLUMN purged_at TEXT;
ALTER TABLE tool_calls ADD COLUMN purged_at TEXT;
ALTER TABLE runs ADD COLUMN retention_state TEXT NOT NULL DEFAULT 'retained';

-- Historical completed/cancelled records predate the durable closure marker.
-- A failed WorkItem is deliberately not backfilled: failures can remain open
-- at a correction boundary unless a controller explicitly seals the closure.
UPDATE work_items
SET closed_at = COALESCE(status_changed_at, updated_at, created_at),
    closure_reason = COALESCE(closure_reason, 'terminal_status_migration_backfill')
WHERE closed_at IS NULL AND status IN ('completed', 'cancelled');

CREATE TRIGGER work_items_terminal_closure_after_insert
AFTER INSERT ON work_items
WHEN NEW.status IN ('completed', 'cancelled') AND NEW.closed_at IS NULL
BEGIN
  UPDATE work_items
  SET closed_at = COALESCE(NEW.status_changed_at, NEW.updated_at, NEW.created_at),
      closure_reason = COALESCE(NEW.closure_reason, 'terminal_status_controller_backfill')
  WHERE id = NEW.id;
END;

CREATE TRIGGER work_items_terminal_closure_after_status_update
AFTER UPDATE OF status ON work_items
WHEN NEW.status IN ('completed', 'cancelled') AND NEW.closed_at IS NULL
BEGIN
  UPDATE work_items
  SET closed_at = COALESCE(NEW.status_changed_at, NEW.updated_at, NEW.created_at),
      closure_reason = COALESCE(NEW.closure_reason, 'terminal_status_controller_backfill')
  WHERE id = NEW.id;
END;

CREATE TRIGGER work_items_closure_cannot_be_cleared
BEFORE UPDATE OF closed_at ON work_items
WHEN OLD.closed_at IS NOT NULL AND NEW.closed_at IS NULL
BEGIN
  SELECT RAISE(ABORT, 'work item closure cannot be cleared');
END;

CREATE TRIGGER repository_binding_revisions_immutable_update
BEFORE UPDATE ON repository_binding_revisions
BEGIN
  SELECT RAISE(ABORT, 'repository binding revisions are immutable');
END;

CREATE TRIGGER repository_binding_revisions_immutable_delete
BEFORE DELETE ON repository_binding_revisions
BEGIN
  SELECT RAISE(ABORT, 'repository binding revisions are immutable');
END;

CREATE TRIGGER product_model_snapshots_immutable_update
BEFORE UPDATE ON product_model_snapshots
BEGIN
  SELECT RAISE(ABORT, 'product model snapshots are immutable');
END;

CREATE TRIGGER product_model_snapshots_immutable_delete
BEFORE DELETE ON product_model_snapshots
BEGIN
  SELECT RAISE(ABORT, 'product model snapshots are immutable');
END;
