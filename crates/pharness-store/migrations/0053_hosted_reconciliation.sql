-- Scheduling belongs to the existing WorkItem. Legacy work is never enrolled.
CREATE TABLE hosted_reconciliations (
  work_item_id TEXT PRIMARY KEY REFERENCES work_items(id),
  control TEXT NOT NULL DEFAULT 'active' CHECK(control IN ('active','paused','cancelled')),
  control_version INTEGER NOT NULL DEFAULT 1,
  next_due_at INTEGER NOT NULL,
  claim_owner TEXT,
  claim_fence INTEGER NOT NULL DEFAULT 0,
  claim_until INTEGER,
  condition TEXT NOT NULL DEFAULT 'ready',
  condition_reason TEXT NOT NULL DEFAULT 'Authorized hosted work is ready for reconciliation',
  unchanged_checks INTEGER NOT NULL DEFAULT 0 CHECK(unchanged_checks >= 0),
  observed_state_hash TEXT,
  updated_at INTEGER NOT NULL,
  CHECK((claim_owner IS NULL) = (claim_until IS NULL))
);
CREATE INDEX hosted_reconciliations_due ON hosted_reconciliations(next_due_at, claim_until);

INSERT INTO hosted_reconciliations(work_item_id, next_due_at, updated_at)
 SELECT id, CAST(created_at AS INTEGER), CAST(updated_at AS INTEGER)
 FROM work_items WHERE workflow_policy_json IS NOT NULL;
CREATE TRIGGER enroll_hosted_workflow AFTER INSERT ON work_items
WHEN NEW.workflow_policy_json IS NOT NULL
BEGIN
  INSERT INTO hosted_reconciliations(work_item_id, next_due_at, updated_at)
  VALUES(NEW.id, CAST(NEW.created_at AS INTEGER), CAST(NEW.created_at AS INTEGER));
END;

-- Operation records retain dispatch identity and references to existing Run,
-- source, pipeline, and deployment intents; they do not duplicate their evidence.
CREATE TABLE hosted_operations (
  id TEXT PRIMARY KEY,
  work_item_id TEXT NOT NULL REFERENCES hosted_reconciliations(work_item_id),
  action TEXT NOT NULL,
  input_hash TEXT NOT NULL,
  effect TEXT NOT NULL CHECK(effect IN ('development','observation','recovery')),
  status TEXT NOT NULL DEFAULT 'pending' CHECK(status IN ('pending','running','blocked','succeeded')),
  resource_refs_json TEXT NOT NULL DEFAULT '{}' CHECK(json_valid(resource_refs_json)),
  status_reason TEXT NOT NULL DEFAULT 'Dispatch identity recorded before execution',
  created_at INTEGER NOT NULL,
  updated_at INTEGER NOT NULL,
  UNIQUE(work_item_id, action, input_hash)
);
CREATE UNIQUE INDEX hosted_one_active_operation ON hosted_operations(work_item_id)
 WHERE status IN ('pending','running','blocked');
CREATE TRIGGER hosted_operation_identity_immutable
BEFORE UPDATE OF id, work_item_id, action, input_hash, effect, created_at ON hosted_operations
BEGIN
  SELECT RAISE(ABORT, 'hosted operation identity is immutable');
END;

-- Locks do not expire with the API claim: an external operation can outlive an
-- API restart. Only a reconciled terminal operation releases its resource locks.
CREATE TABLE hosted_operation_locks (
  resource_key TEXT PRIMARY KEY,
  operation_id TEXT NOT NULL REFERENCES hosted_operations(id)
);
