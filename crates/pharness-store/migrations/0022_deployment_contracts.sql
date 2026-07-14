CREATE TABLE deployment_contracts (
  id TEXT PRIMARY KEY,
  status TEXT NOT NULL,
  target_environment TEXT NOT NULL,
  target_namespace TEXT NOT NULL,
  argo_application TEXT NOT NULL,
  version TEXT NOT NULL,
  contract_json TEXT NOT NULL,
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL,
  status_changed_at TEXT NOT NULL,
  status_changed_by TEXT,
  status_reason TEXT,
  UNIQUE(target_environment, target_namespace, argo_application, version)
);

CREATE INDEX idx_deployment_contracts_lookup
  ON deployment_contracts(
    target_environment,
    target_namespace,
    argo_application,
    status,
    created_at DESC
  );

CREATE UNIQUE INDEX idx_deployment_contracts_one_active_target
  ON deployment_contracts(target_environment, target_namespace, argo_application)
  WHERE status = 'active';
