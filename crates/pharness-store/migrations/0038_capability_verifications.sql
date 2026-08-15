CREATE TABLE capability_verifications (
  id TEXT PRIMARY KEY NOT NULL,
  capability TEXT NOT NULL,
  status TEXT NOT NULL CHECK (status IN ('available', 'unavailable')),
  summary TEXT NOT NULL,
  principal TEXT,
  repository TEXT,
  permission TEXT,
  verified_at TEXT NOT NULL,
  expires_at TEXT NOT NULL
);

CREATE INDEX idx_capability_verifications_latest
  ON capability_verifications(capability, verified_at DESC, id DESC);
