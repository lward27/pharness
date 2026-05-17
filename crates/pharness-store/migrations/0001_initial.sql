CREATE TABLE sessions (
  id TEXT PRIMARY KEY,
  title TEXT NOT NULL,
  cwd TEXT NOT NULL,
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL,
  archived_at TEXT,
  metadata_json TEXT NOT NULL DEFAULT '{}'
);

CREATE TABLE environments (
  id TEXT PRIMARY KEY,
  name TEXT NOT NULL,
  tier TEXT NOT NULL,
  cluster TEXT,
  namespace TEXT,
  metadata_json TEXT NOT NULL DEFAULT '{}',
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL
);

CREATE TABLE runs (
  id TEXT PRIMARY KEY,
  session_id TEXT NOT NULL REFERENCES sessions(id),
  environment_id TEXT REFERENCES environments(id),
  status TEXT NOT NULL,
  user_task TEXT NOT NULL,
  max_turns INTEGER NOT NULL,
  started_at TEXT NOT NULL,
  finished_at TEXT,
  cancel_requested_at TEXT,
  error TEXT,
  execution_target_json TEXT NOT NULL DEFAULT '{"kind":"local_process"}',
  metadata_json TEXT NOT NULL DEFAULT '{}'
);

CREATE TABLE messages (
  id TEXT PRIMARY KEY,
  session_id TEXT NOT NULL REFERENCES sessions(id),
  run_id TEXT REFERENCES runs(id),
  role TEXT NOT NULL,
  content TEXT NOT NULL,
  created_at TEXT NOT NULL,
  token_estimate INTEGER,
  metadata_json TEXT NOT NULL DEFAULT '{}'
);

CREATE TABLE events (
  id TEXT PRIMARY KEY,
  session_id TEXT NOT NULL REFERENCES sessions(id),
  run_id TEXT REFERENCES runs(id),
  seq INTEGER NOT NULL,
  type TEXT NOT NULL,
  ts TEXT NOT NULL,
  payload_json TEXT NOT NULL,
  UNIQUE(run_id, seq)
);

CREATE TABLE tool_calls (
  id TEXT PRIMARY KEY,
  session_id TEXT NOT NULL REFERENCES sessions(id),
  run_id TEXT NOT NULL REFERENCES runs(id),
  action_id TEXT NOT NULL,
  action_type TEXT NOT NULL,
  status TEXT NOT NULL,
  approval_id TEXT,
  proposed_at TEXT NOT NULL,
  started_at TEXT,
  finished_at TEXT,
  args_json TEXT NOT NULL,
  result_json TEXT,
  policy_json TEXT NOT NULL
);

CREATE TABLE approvals (
  id TEXT PRIMARY KEY,
  session_id TEXT NOT NULL REFERENCES sessions(id),
  run_id TEXT NOT NULL REFERENCES runs(id),
  tool_call_id TEXT REFERENCES tool_calls(id),
  status TEXT NOT NULL,
  kind TEXT NOT NULL,
  summary TEXT NOT NULL,
  risk_level TEXT NOT NULL,
  requested_at TEXT NOT NULL,
  decided_at TEXT,
  decided_by TEXT,
  decision_reason TEXT
);

CREATE TABLE artifacts (
  id TEXT PRIMARY KEY,
  session_id TEXT NOT NULL REFERENCES sessions(id),
  run_id TEXT REFERENCES runs(id),
  kind TEXT NOT NULL,
  label TEXT NOT NULL,
  mime_type TEXT,
  path TEXT,
  content_text TEXT,
  content_json TEXT,
  created_at TEXT NOT NULL
);

CREATE TABLE file_changes (
  id TEXT PRIMARY KEY,
  session_id TEXT NOT NULL REFERENCES sessions(id),
  run_id TEXT NOT NULL REFERENCES runs(id),
  tool_call_id TEXT REFERENCES tool_calls(id),
  path TEXT NOT NULL,
  before_hash TEXT,
  after_hash TEXT,
  diff TEXT NOT NULL,
  created_at TEXT NOT NULL
);

CREATE TABLE resource_refs (
  id TEXT PRIMARY KEY,
  session_id TEXT NOT NULL REFERENCES sessions(id),
  run_id TEXT REFERENCES runs(id),
  provider TEXT NOT NULL,
  kind TEXT NOT NULL,
  name TEXT NOT NULL,
  namespace TEXT,
  uri TEXT,
  metadata_json TEXT NOT NULL DEFAULT '{}',
  observed_at TEXT NOT NULL
);

CREATE TABLE context_items (
  id TEXT PRIMARY KEY,
  session_id TEXT NOT NULL REFERENCES sessions(id),
  run_id TEXT REFERENCES runs(id),
  source TEXT NOT NULL,
  kind TEXT NOT NULL,
  title TEXT NOT NULL,
  content TEXT NOT NULL,
  resource_ref_id TEXT REFERENCES resource_refs(id),
  token_estimate INTEGER,
  metadata_json TEXT NOT NULL DEFAULT '{}',
  created_at TEXT NOT NULL
);

CREATE INDEX idx_runs_session_started ON runs(session_id, started_at DESC);
CREATE INDEX idx_events_run_seq ON events(run_id, seq);
CREATE INDEX idx_approvals_status ON approvals(status, requested_at DESC);
CREATE INDEX idx_artifacts_run ON artifacts(run_id, created_at DESC);
CREATE INDEX idx_resource_refs_run ON resource_refs(run_id, provider, kind);
CREATE INDEX idx_context_items_session ON context_items(session_id, created_at DESC);
