-- Campus OIDC identities, tenant administrators, course memberships,
-- one-time login attempts and server-side browser sessions.
--
-- The identity key is always (tenant_id, issuer, subject). Email and display
-- name are display-only. Memberships and administrators reference the same
-- tuple so that manual enrollment can precede the first login. Secrets are
-- never stored: sessions, CSRF tokens, login states and browser bindings are
-- SHA-256 hashes. All timestamps are Unix seconds.

CREATE TABLE campus_identities (
  id TEXT PRIMARY KEY,
  tenant_id TEXT NOT NULL CHECK (length(tenant_id) BETWEEN 1 AND 64),
  issuer TEXT NOT NULL CHECK (length(issuer) BETWEEN 1 AND 512),
  subject TEXT NOT NULL CHECK (length(subject) BETWEEN 1 AND 255),
  email TEXT,
  display_name TEXT,
  created_at INTEGER NOT NULL,
  updated_at INTEGER NOT NULL,
  last_login_at INTEGER NOT NULL,
  UNIQUE (tenant_id, issuer, subject)
);

CREATE TABLE campus_tenant_admins (
  tenant_id TEXT NOT NULL,
  issuer TEXT NOT NULL,
  subject TEXT NOT NULL,
  source TEXT NOT NULL CHECK (source IN ('bootstrap')),
  created_at INTEGER NOT NULL,
  PRIMARY KEY (tenant_id, issuer, subject)
);

CREATE TABLE campus_course_memberships (
  tenant_id TEXT NOT NULL,
  course_id TEXT NOT NULL CHECK (length(course_id) BETWEEN 1 AND 128),
  issuer TEXT NOT NULL,
  subject TEXT NOT NULL,
  role TEXT NOT NULL CHECK (role IN ('student', 'instructor')),
  granted_by_identity_id TEXT,
  created_at INTEGER NOT NULL,
  updated_at INTEGER NOT NULL,
  PRIMARY KEY (tenant_id, course_id, issuer, subject)
);

CREATE INDEX campus_course_memberships_identity
  ON campus_course_memberships (tenant_id, issuer, subject);

CREATE TABLE campus_login_attempts (
  state_hash TEXT PRIMARY KEY,
  tenant_id TEXT NOT NULL,
  browser_binding_hash TEXT NOT NULL,
  nonce TEXT NOT NULL,
  pkce_verifier TEXT NOT NULL,
  created_at INTEGER NOT NULL,
  expires_at INTEGER NOT NULL,
  consumed_at INTEGER
);

CREATE INDEX campus_login_attempts_expiry ON campus_login_attempts (expires_at);

CREATE TABLE campus_sessions (
  id_hash TEXT PRIMARY KEY,
  tenant_id TEXT NOT NULL,
  identity_id TEXT NOT NULL REFERENCES campus_identities (id),
  csrf_hash TEXT NOT NULL,
  created_at INTEGER NOT NULL,
  last_seen_at INTEGER NOT NULL,
  idle_expires_at INTEGER NOT NULL,
  absolute_expires_at INTEGER NOT NULL,
  revoked_at INTEGER,
  CHECK (idle_expires_at <= absolute_expires_at)
);

CREATE INDEX campus_sessions_identity ON campus_sessions (tenant_id, identity_id);
CREATE INDEX campus_sessions_expiry ON campus_sessions (absolute_expires_at);
