-- Campus broker tokens: short-lived, audience-scoped credentials that a
-- student sandbox presents to trusted broker endpoints instead of any worker,
-- operator, provider or cluster credential.
--
-- Only a domain-separated SHA-256 of each token is stored. A token names one
-- audience, one lab session, the slot occupancy (slot_id, slot_generation)
-- and the lab generation it was issued for; the broker re-checks all of them
-- against the journal on every request.
--
-- The table is additive. The one trigger on an existing table only writes to
-- this table: whenever a session leaves `provisioning`/`ready`, is asked to
-- stop or is revoked, its live tokens are revoked in the same transaction.

CREATE TABLE campus_broker_tokens (
  sequence INTEGER PRIMARY KEY AUTOINCREMENT,
  token_id TEXT NOT NULL UNIQUE CHECK (length(token_id) BETWEEN 1 AND 128),
  token_hash TEXT NOT NULL UNIQUE CHECK (length(token_hash) = 64),
  audience TEXT NOT NULL CHECK (audience IN ('inference', 'tools')),
  session_id TEXT NOT NULL REFERENCES campus_sessions_journal (session_id),
  slot_id TEXT NOT NULL REFERENCES campus_lab_slots (slot_id),
  slot_generation INTEGER NOT NULL CHECK (slot_generation >= 1),
  lab_id TEXT NOT NULL REFERENCES campus_labs (lab_id),
  lab_generation INTEGER NOT NULL CHECK (lab_generation >= 1),
  issued_by TEXT NOT NULL CHECK (length(issued_by) >= 1),
  issued_at INTEGER NOT NULL,
  -- At most 15 minutes, matching CAMPUS_BROKER_TOKEN_MAX_TTL_SECONDS.
  expires_at INTEGER NOT NULL CHECK (expires_at > issued_at AND expires_at - issued_at <= 900),
  revoked_at INTEGER,
  revoked_reason TEXT CHECK (revoked_reason IS NULL OR length(revoked_reason) BETWEEN 1 AND 64),
  CHECK ((revoked_at IS NULL) = (revoked_reason IS NULL))
);

CREATE INDEX campus_broker_tokens_live
  ON campus_broker_tokens (session_id, audience, sequence) WHERE revoked_at IS NULL;

-- A token never changes except to be revoked, once.
CREATE TRIGGER campus_broker_tokens_immutable
BEFORE UPDATE ON campus_broker_tokens
WHEN NOT (
  OLD.revoked_at IS NULL AND NEW.revoked_at IS NOT NULL
  AND NEW.sequence IS OLD.sequence
  AND NEW.token_id IS OLD.token_id
  AND NEW.token_hash IS OLD.token_hash
  AND NEW.audience IS OLD.audience
  AND NEW.session_id IS OLD.session_id
  AND NEW.slot_id IS OLD.slot_id
  AND NEW.slot_generation IS OLD.slot_generation
  AND NEW.lab_id IS OLD.lab_id
  AND NEW.lab_generation IS OLD.lab_generation
  AND NEW.issued_by IS OLD.issued_by
  AND NEW.issued_at IS OLD.issued_at
  AND NEW.expires_at IS OLD.expires_at
)
BEGIN
  SELECT RAISE(ABORT, 'campus broker tokens are immutable except for one revocation');
END;

CREATE TRIGGER campus_broker_tokens_no_delete
BEFORE DELETE ON campus_broker_tokens
BEGIN
  SELECT RAISE(ABORT, 'campus broker tokens are never deleted');
END;

CREATE TRIGGER campus_broker_tokens_revoke_with_session
AFTER UPDATE ON campus_sessions_journal
WHEN NEW.desired_state = 'terminated'
  OR NEW.revoked_at IS NOT NULL
  OR NEW.state NOT IN ('provisioning', 'ready')
BEGIN
  UPDATE campus_broker_tokens
  SET revoked_at = NEW.updated_at,
      revoked_reason = CASE
        WHEN NEW.state IN ('provisioning', 'ready') THEN 'session_stop_requested'
        ELSE 'session_' || NEW.state
      END
  WHERE session_id = NEW.session_id AND revoked_at IS NULL;
END;
