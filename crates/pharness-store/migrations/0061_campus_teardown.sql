-- Campus teardown (LUC-133): slot holds and teardown hook completions.
--
-- Both tables are additive; no existing table, index or trigger changes.
--
-- A slot hold keeps a lab slot out of allocation and out of the tenant's
-- admission capacity without touching campus_lab_slots, whose fencing
-- trigger stays as it is:
-- - `quarantined`: the janitor (or an operator) found provider resources in
--   the slot that no live occupancy accounts for and could not confirm their
--   removal. Nothing is force-deleted; the hold stays until an inventory of
--   the slot comes back clean.
-- - `retired`: the slot is registered but no longer configured. A current
--   occupant keeps running until it ends; the slot is then never reused.
-- At most one hold per slot is active. A hold is released once and never
-- deleted, so the table is also the audit trail.
--
-- A teardown hook run records that a named hook released whatever a
-- session's workload held outside the execution provider, so a replayed or
-- retried cleanup does not call it again.

CREATE TABLE campus_lab_slot_holds (
  sequence INTEGER PRIMARY KEY AUTOINCREMENT,
  slot_id TEXT NOT NULL REFERENCES campus_lab_slots (slot_id),
  kind TEXT NOT NULL CHECK (kind IN ('quarantined', 'retired')),
  reason TEXT NOT NULL CHECK (length(reason) BETWEEN 1 AND 512),
  held_by TEXT NOT NULL CHECK (length(held_by) BETWEEN 1 AND 128),
  held_at INTEGER NOT NULL,
  released_by TEXT CHECK (released_by IS NULL OR length(released_by) BETWEEN 1 AND 128),
  released_at INTEGER,
  release_reason TEXT CHECK (release_reason IS NULL OR length(release_reason) BETWEEN 1 AND 512),
  CHECK ((released_at IS NULL) = (released_by IS NULL)),
  CHECK ((released_at IS NULL) = (release_reason IS NULL)),
  CHECK (released_at IS NULL OR released_at >= held_at)
);

CREATE UNIQUE INDEX campus_lab_slot_holds_active
  ON campus_lab_slot_holds (slot_id) WHERE released_at IS NULL;

CREATE TRIGGER campus_lab_slot_holds_release_once
BEFORE UPDATE ON campus_lab_slot_holds
WHEN NOT (
  OLD.released_at IS NULL AND NEW.released_at IS NOT NULL
  AND NEW.sequence IS OLD.sequence
  AND NEW.slot_id IS OLD.slot_id
  AND NEW.kind IS OLD.kind
  AND NEW.reason IS OLD.reason
  AND NEW.held_by IS OLD.held_by
  AND NEW.held_at IS OLD.held_at
)
BEGIN
  SELECT RAISE(ABORT, 'campus lab slot holds change only by being released, once');
END;

CREATE TRIGGER campus_lab_slot_holds_no_delete
BEFORE DELETE ON campus_lab_slot_holds
BEGIN
  SELECT RAISE(ABORT, 'campus lab slot holds are never deleted');
END;

CREATE TABLE campus_teardown_hook_runs (
  session_id TEXT NOT NULL REFERENCES campus_sessions_journal (session_id),
  hook TEXT NOT NULL CHECK (length(hook) BETWEEN 1 AND 64),
  op_key TEXT NOT NULL CHECK (length(op_key) >= 1),
  slot_id TEXT NOT NULL REFERENCES campus_lab_slots (slot_id),
  slot_generation INTEGER NOT NULL CHECK (slot_generation >= 1),
  completed_at INTEGER NOT NULL,
  PRIMARY KEY (session_id, hook)
);

CREATE TRIGGER campus_teardown_hook_runs_immutable
BEFORE UPDATE ON campus_teardown_hook_runs
BEGIN
  SELECT RAISE(ABORT, 'campus teardown hook runs are immutable');
END;

CREATE TRIGGER campus_teardown_hook_runs_no_delete
BEFORE DELETE ON campus_teardown_hook_runs
BEGIN
  SELECT RAISE(ABORT, 'campus teardown hook runs are never deleted');
END;
