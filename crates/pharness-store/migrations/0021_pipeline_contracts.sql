CREATE TABLE pipeline_contracts (
  id TEXT PRIMARY KEY,
  status TEXT NOT NULL,
  namespace TEXT NOT NULL,
  pipeline_ref TEXT NOT NULL,
  version TEXT NOT NULL,
  contract_json TEXT NOT NULL,
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL,
  status_changed_at TEXT NOT NULL,
  status_changed_by TEXT,
  status_reason TEXT,
  UNIQUE(namespace, pipeline_ref, version)
);

CREATE INDEX idx_pipeline_contracts_lookup
  ON pipeline_contracts(namespace, pipeline_ref, status, created_at DESC);
