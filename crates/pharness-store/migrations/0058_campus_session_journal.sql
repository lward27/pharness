-- Campus lab session journal: the durable local state machine for student
-- lab sessions, their provider operations, an append-only audit outbox and
-- fenced lab slots.
--
-- These tables are additive and reference no SDLC table. Lab sessions are
-- keyed by the Campus identity tuple (tenant_id, issuer, subject) and record
-- the Campus profile id, version and hash they launched with. Timestamps are
-- Unix seconds.
--
-- Fencing:
-- - campus_sessions_journal.generation increases by exactly one on every
--   state change; writers compare-and-set on (session_id, generation).
-- - campus_lab_slots.occupant_generation increases by exactly one each time
--   a free slot gets an occupant. Provider resources carry it, so a stale
--   occupancy can never act on a newer one.
-- - campus_session_operations.lease_fence increases on every lease; only the
--   live holder of the latest fence may complete an operation.

-- The transition table of CampusLabSessionState::successors. A trigger on the
-- journal rejects every state change that is not listed here.
CREATE TABLE campus_session_transitions (
  from_state TEXT NOT NULL,
  to_state TEXT NOT NULL,
  PRIMARY KEY (from_state, to_state)
) WITHOUT ROWID;

INSERT INTO campus_session_transitions (from_state, to_state) VALUES
  ('requested', 'authorizing'),
  ('requested', 'terminated'),
  ('authorizing', 'provisioning'),
  ('authorizing', 'failed'),
  ('authorizing', 'terminated'),
  ('provisioning', 'ready'),
  ('provisioning', 'stopping'),
  ('ready', 'stopping'),
  ('stopping', 'cleaning'),
  ('cleaning', 'terminated'),
  ('cleaning', 'failed'),
  ('cleaning', 'cleanup_blocked'),
  ('cleanup_blocked', 'cleaning');

CREATE TRIGGER campus_session_transitions_immutable_update
BEFORE UPDATE ON campus_session_transitions
BEGIN
  SELECT RAISE(ABORT, 'campus session transitions are fixed by migration');
END;

CREATE TRIGGER campus_session_transitions_immutable_insert
BEFORE INSERT ON campus_session_transitions
BEGIN
  SELECT RAISE(ABORT, 'campus session transitions are fixed by migration');
END;

CREATE TRIGGER campus_session_transitions_immutable_delete
BEFORE DELETE ON campus_session_transitions
BEGIN
  SELECT RAISE(ABORT, 'campus session transitions are fixed by migration');
END;

CREATE TABLE campus_lab_slots (
  slot_id TEXT PRIMARY KEY CHECK (length(slot_id) BETWEEN 1 AND 128),
  tenant_id TEXT NOT NULL CHECK (length(tenant_id) BETWEEN 1 AND 64),
  occupant_session_id TEXT REFERENCES campus_sessions_journal (session_id),
  occupant_generation INTEGER NOT NULL DEFAULT 0 CHECK (occupant_generation >= 0),
  occupied_at INTEGER,
  created_at INTEGER NOT NULL,
  updated_at INTEGER NOT NULL,
  CHECK ((occupant_session_id IS NULL) = (occupied_at IS NULL))
);

CREATE UNIQUE INDEX campus_lab_slots_one_per_session
  ON campus_lab_slots (occupant_session_id) WHERE occupant_session_id IS NOT NULL;
CREATE INDEX campus_lab_slots_free ON campus_lab_slots (tenant_id, occupant_session_id, slot_id);

-- A slot changes occupant only by being freed and then taken, and taking it
-- advances the generation by exactly one.
CREATE TRIGGER campus_lab_slots_fenced
BEFORE UPDATE ON campus_lab_slots
WHEN NEW.slot_id IS NOT OLD.slot_id
  OR NEW.tenant_id IS NOT OLD.tenant_id
  OR NEW.created_at IS NOT OLD.created_at
  OR CASE
       WHEN OLD.occupant_session_id IS NULL AND NEW.occupant_session_id IS NOT NULL
         THEN NEW.occupant_generation != OLD.occupant_generation + 1
       ELSE NEW.occupant_generation != OLD.occupant_generation
         OR (OLD.occupant_session_id IS NOT NULL AND NEW.occupant_session_id IS NOT NULL
             AND NEW.occupant_session_id != OLD.occupant_session_id)
     END
BEGIN
  SELECT RAISE(ABORT, 'campus lab slot occupancy is fenced');
END;

CREATE TABLE campus_sessions_journal (
  session_id TEXT PRIMARY KEY CHECK (length(session_id) BETWEEN 1 AND 128),
  tenant_id TEXT NOT NULL,
  course_id TEXT NOT NULL CHECK (length(course_id) BETWEEN 1 AND 128),
  issuer TEXT NOT NULL,
  subject TEXT NOT NULL,
  profile_id TEXT NOT NULL CHECK (length(profile_id) BETWEEN 1 AND 64),
  profile_version TEXT NOT NULL CHECK (length(profile_version) BETWEEN 1 AND 64),
  profile_hash TEXT NOT NULL CHECK (length(profile_hash) BETWEEN 1 AND 128),
  state TEXT NOT NULL DEFAULT 'requested' CHECK (state IN (
    'requested', 'authorizing', 'provisioning', 'ready', 'stopping', 'cleaning',
    'cleanup_blocked', 'terminated', 'failed'
  )),
  desired_state TEXT NOT NULL DEFAULT 'ready' CHECK (desired_state IN ('ready', 'terminated')),
  observed_state TEXT NOT NULL DEFAULT 'unknown' CHECK (observed_state IN (
    'unknown', 'absent', 'provisioning', 'ready', 'deleting', 'deleted', 'failed', 'unreachable'
  )),
  generation INTEGER NOT NULL DEFAULT 1 CHECK (generation >= 1),
  slot_id TEXT REFERENCES campus_lab_slots (slot_id),
  slot_generation INTEGER CHECK (slot_generation >= 1),
  workspace_id TEXT,
  failure_reason TEXT,
  blocked_reason TEXT,
  created_by TEXT NOT NULL CHECK (length(created_by) >= 1),
  created_at INTEGER NOT NULL,
  updated_at INTEGER NOT NULL,
  observed_at INTEGER,
  revoked_at INTEGER,
  quarantined_at INTEGER,
  terminated_at INTEGER,
  expires_at INTEGER NOT NULL CHECK (expires_at > created_at),
  FOREIGN KEY (tenant_id, issuer, subject)
    REFERENCES campus_identities (tenant_id, issuer, subject),
  CHECK ((slot_id IS NULL) = (slot_generation IS NULL)),
  CHECK ((state IN ('provisioning', 'ready', 'stopping', 'cleaning', 'cleanup_blocked'))
    <= (slot_id IS NOT NULL)),
  CHECK ((state IN ('stopping', 'cleaning', 'cleanup_blocked')) <= (revoked_at IS NOT NULL)),
  CHECK ((state = 'ready') <= (workspace_id IS NOT NULL)),
  CHECK ((state = 'cleanup_blocked') = (quarantined_at IS NOT NULL)),
  CHECK ((state = 'cleanup_blocked') <= (blocked_reason IS NOT NULL)),
  CHECK ((state IN ('terminated', 'failed')) = (terminated_at IS NOT NULL)),
  CHECK ((state = 'failed') <= (failure_reason IS NOT NULL))
);

CREATE INDEX campus_sessions_journal_identity
  ON campus_sessions_journal (tenant_id, issuer, subject);
CREATE INDEX campus_sessions_journal_state ON campus_sessions_journal (tenant_id, state);

CREATE TRIGGER campus_sessions_journal_transition
BEFORE UPDATE OF state ON campus_sessions_journal
WHEN NEW.state IS NOT OLD.state AND NOT EXISTS (
  SELECT 1 FROM campus_session_transitions
  WHERE from_state = OLD.state AND to_state = NEW.state
)
BEGIN
  SELECT RAISE(ABORT, 'illegal campus session transition');
END;

CREATE TRIGGER campus_sessions_journal_fenced
BEFORE UPDATE ON campus_sessions_journal
WHEN NEW.session_id IS NOT OLD.session_id
  OR NEW.tenant_id IS NOT OLD.tenant_id
  OR NEW.course_id IS NOT OLD.course_id
  OR NEW.issuer IS NOT OLD.issuer
  OR NEW.subject IS NOT OLD.subject
  OR NEW.profile_id IS NOT OLD.profile_id
  OR NEW.profile_version IS NOT OLD.profile_version
  OR NEW.profile_hash IS NOT OLD.profile_hash
  OR NEW.created_by IS NOT OLD.created_by
  OR NEW.created_at IS NOT OLD.created_at
  OR NEW.expires_at IS NOT OLD.expires_at
  OR (NEW.state IS NOT OLD.state AND NEW.generation != OLD.generation + 1)
  OR (NEW.state IS OLD.state AND NEW.generation != OLD.generation)
  OR (OLD.slot_id IS NOT NULL
      AND (NEW.slot_id IS NOT OLD.slot_id OR NEW.slot_generation IS NOT OLD.slot_generation))
  OR (OLD.desired_state = 'terminated' AND NEW.desired_state != 'terminated')
  OR OLD.state IN ('terminated', 'failed')
BEGIN
  SELECT RAISE(ABORT, 'campus session journal write is not fenced');
END;

CREATE TABLE campus_session_operations (
  op_key TEXT PRIMARY KEY CHECK (length(op_key) BETWEEN 1 AND 256),
  session_id TEXT NOT NULL REFERENCES campus_sessions_journal (session_id),
  kind TEXT NOT NULL CHECK (kind IN ('provision', 'delete')),
  session_generation INTEGER NOT NULL CHECK (session_generation >= 1),
  slot_id TEXT NOT NULL REFERENCES campus_lab_slots (slot_id),
  slot_generation INTEGER NOT NULL CHECK (slot_generation >= 1),
  status TEXT NOT NULL DEFAULT 'pending' CHECK (status IN (
    'pending', 'succeeded', 'failed', 'blocked', 'abandoned'
  )),
  attempt_count INTEGER NOT NULL DEFAULT 0 CHECK (attempt_count >= 0),
  lease_owner TEXT,
  lease_expires_at INTEGER,
  lease_fence INTEGER NOT NULL DEFAULT 0 CHECK (lease_fence >= 0),
  last_error TEXT,
  result_json TEXT NOT NULL DEFAULT '{}' CHECK (json_valid(result_json)),
  created_at INTEGER NOT NULL,
  updated_at INTEGER NOT NULL,
  completed_at INTEGER,
  UNIQUE (session_id, kind, session_generation),
  CHECK ((lease_owner IS NULL) = (lease_expires_at IS NULL)),
  CHECK ((status = 'pending') = (completed_at IS NULL)),
  CHECK (status = 'pending' OR lease_owner IS NULL)
);

CREATE UNIQUE INDEX campus_session_operations_one_pending
  ON campus_session_operations (session_id) WHERE status = 'pending';

CREATE TRIGGER campus_session_operations_fenced
BEFORE UPDATE ON campus_session_operations
WHEN NEW.op_key IS NOT OLD.op_key
  OR NEW.session_id IS NOT OLD.session_id
  OR NEW.kind IS NOT OLD.kind
  OR NEW.session_generation IS NOT OLD.session_generation
  OR NEW.slot_id IS NOT OLD.slot_id
  OR NEW.slot_generation IS NOT OLD.slot_generation
  OR NEW.created_at IS NOT OLD.created_at
  OR OLD.status != 'pending'
  OR NEW.lease_fence < OLD.lease_fence
  OR NEW.attempt_count < OLD.attempt_count
BEGIN
  SELECT RAISE(ABORT, 'campus session operation identity is immutable and completion is final');
END;

-- Append-only audit log and outbox. Only published_at may be set, once.
CREATE TABLE campus_session_events (
  sequence INTEGER PRIMARY KEY AUTOINCREMENT,
  session_id TEXT NOT NULL REFERENCES campus_sessions_journal (session_id),
  kind TEXT NOT NULL CHECK (length(kind) BETWEEN 1 AND 128),
  from_state TEXT,
  to_state TEXT,
  generation INTEGER NOT NULL CHECK (generation >= 1),
  op_key TEXT REFERENCES campus_session_operations (op_key),
  actor TEXT NOT NULL CHECK (length(actor) >= 1),
  payload_json TEXT NOT NULL DEFAULT '{}' CHECK (json_valid(payload_json)),
  created_at INTEGER NOT NULL,
  published_at INTEGER
);

CREATE INDEX campus_session_events_session ON campus_session_events (session_id, sequence);
CREATE INDEX campus_session_events_unpublished
  ON campus_session_events (sequence) WHERE published_at IS NULL;

CREATE TRIGGER campus_session_events_append_only
BEFORE UPDATE ON campus_session_events
WHEN NOT (
  OLD.published_at IS NULL AND NEW.published_at IS NOT NULL
  AND NEW.sequence IS OLD.sequence
  AND NEW.session_id IS OLD.session_id
  AND NEW.kind IS OLD.kind
  AND NEW.from_state IS OLD.from_state
  AND NEW.to_state IS OLD.to_state
  AND NEW.generation IS OLD.generation
  AND NEW.op_key IS OLD.op_key
  AND NEW.actor IS OLD.actor
  AND NEW.payload_json IS OLD.payload_json
  AND NEW.created_at IS OLD.created_at
)
BEGIN
  SELECT RAISE(ABORT, 'campus session events are append-only');
END;

CREATE TRIGGER campus_session_events_no_delete
BEFORE DELETE ON campus_session_events
BEGIN
  SELECT RAISE(ABORT, 'campus session events are append-only');
END;
