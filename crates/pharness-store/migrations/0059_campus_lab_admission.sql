-- Campus lab admission (assignment labs, reset generations and launch
-- idempotency keys) on top of the lab session journal (0058).
--
-- A lab is one student's workspace for one assignment of one course. It has
-- a stable lab_id and a budget_id that never change, so every session of the
-- lab, including every reset, is charged to the same budget. Each session
-- the lab runs is one journal row and one generation; generation N+1 is only
-- created once generation N is terminal.
--
-- The tables are additive: no existing table, index or trigger is changed.
-- Admission runs under BEGIN IMMEDIATE, so the counts it checks and the rows
-- it writes cannot interleave with another launch.

CREATE TABLE campus_labs (
  lab_id TEXT PRIMARY KEY CHECK (length(lab_id) BETWEEN 1 AND 128),
  tenant_id TEXT NOT NULL CHECK (length(tenant_id) BETWEEN 1 AND 64),
  course_id TEXT NOT NULL CHECK (length(course_id) BETWEEN 1 AND 128),
  assignment_id TEXT NOT NULL CHECK (length(assignment_id) BETWEEN 1 AND 128),
  issuer TEXT NOT NULL,
  subject TEXT NOT NULL,
  budget_id TEXT NOT NULL UNIQUE CHECK (length(budget_id) BETWEEN 1 AND 128),
  generation INTEGER NOT NULL DEFAULT 0 CHECK (generation >= 0),
  current_session_id TEXT REFERENCES campus_sessions_journal (session_id),
  reset_requested_at INTEGER,
  created_at INTEGER NOT NULL,
  updated_at INTEGER NOT NULL,
  UNIQUE (tenant_id, course_id, assignment_id, issuer, subject),
  FOREIGN KEY (tenant_id, issuer, subject)
    REFERENCES campus_identities (tenant_id, issuer, subject),
  CHECK ((generation = 0) = (current_session_id IS NULL))
);

-- The owner, assignment and budget of a lab never change, and the current
-- session only changes together with a generation increase of exactly one.
CREATE TRIGGER campus_labs_fenced
BEFORE UPDATE ON campus_labs
WHEN NEW.lab_id IS NOT OLD.lab_id
  OR NEW.tenant_id IS NOT OLD.tenant_id
  OR NEW.course_id IS NOT OLD.course_id
  OR NEW.assignment_id IS NOT OLD.assignment_id
  OR NEW.issuer IS NOT OLD.issuer
  OR NEW.subject IS NOT OLD.subject
  OR NEW.budget_id IS NOT OLD.budget_id
  OR NEW.created_at IS NOT OLD.created_at
  OR (NEW.current_session_id IS NOT OLD.current_session_id
      AND NEW.generation != OLD.generation + 1)
  OR (NEW.current_session_id IS OLD.current_session_id
      AND NEW.generation != OLD.generation)
BEGIN
  SELECT RAISE(ABORT, 'campus lab identity, budget and generation are fenced');
END;

CREATE TRIGGER campus_labs_no_delete
BEFORE DELETE ON campus_labs
BEGIN
  SELECT RAISE(ABORT, 'campus labs are never deleted');
END;

-- One row per session a lab ran, in generation order. Append-only.
CREATE TABLE campus_lab_generations (
  session_id TEXT PRIMARY KEY REFERENCES campus_sessions_journal (session_id),
  lab_id TEXT NOT NULL REFERENCES campus_labs (lab_id),
  generation INTEGER NOT NULL CHECK (generation >= 1),
  effective_policy_hash TEXT NOT NULL CHECK (length(effective_policy_hash) BETWEEN 1 AND 128),
  reset_from_session_id TEXT REFERENCES campus_sessions_journal (session_id),
  created_at INTEGER NOT NULL,
  UNIQUE (lab_id, generation)
);

CREATE INDEX campus_lab_generations_reset_from
  ON campus_lab_generations (reset_from_session_id) WHERE reset_from_session_id IS NOT NULL;

CREATE TRIGGER campus_lab_generations_append_only
BEFORE UPDATE ON campus_lab_generations
BEGIN
  SELECT RAISE(ABORT, 'campus lab generations are append-only');
END;

CREATE TRIGGER campus_lab_generations_no_delete
BEFORE DELETE ON campus_lab_generations
BEGIN
  SELECT RAISE(ABORT, 'campus lab generations are append-only');
END;

-- Client idempotency keys for launches, scoped to the authenticated identity.
-- A key always answers with the session it first produced.
CREATE TABLE campus_launch_requests (
  tenant_id TEXT NOT NULL,
  issuer TEXT NOT NULL,
  subject TEXT NOT NULL,
  idempotency_key TEXT NOT NULL CHECK (length(idempotency_key) BETWEEN 1 AND 128),
  course_id TEXT NOT NULL,
  assignment_id TEXT NOT NULL,
  session_id TEXT NOT NULL REFERENCES campus_sessions_journal (session_id),
  created_at INTEGER NOT NULL,
  PRIMARY KEY (tenant_id, issuer, subject, idempotency_key)
) WITHOUT ROWID;

CREATE TRIGGER campus_launch_requests_immutable
BEFORE UPDATE ON campus_launch_requests
BEGIN
  SELECT RAISE(ABORT, 'campus launch requests are immutable');
END;

-- Admission counts live sessions per course and finds expired ones.
CREATE INDEX campus_sessions_journal_course_state
  ON campus_sessions_journal (tenant_id, course_id, state);
CREATE INDEX campus_sessions_journal_expiry
  ON campus_sessions_journal (tenant_id, expires_at);
