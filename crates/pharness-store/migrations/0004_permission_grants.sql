CREATE TABLE permission_grants (
  id TEXT PRIMARY KEY,
  subject TEXT NOT NULL,
  status TEXT NOT NULL,
  reason TEXT NOT NULL,
  scope_json TEXT NOT NULL,
  policy_json TEXT NOT NULL,
  created_at TEXT NOT NULL,
  expires_at TEXT,
  revoked_at TEXT,
  revoked_by TEXT,
  revoke_reason TEXT
);

CREATE INDEX idx_permission_grants_status_created
ON permission_grants(status, created_at DESC);
