CREATE TABLE organizations (
  id TEXT PRIMARY KEY,
  organization_key TEXT NOT NULL UNIQUE,
  display_name TEXT NOT NULL,
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL
);

CREATE TABLE products (
  id TEXT PRIMARY KEY,
  organization_id TEXT NOT NULL REFERENCES organizations(id),
  product_key TEXT NOT NULL,
  display_name TEXT NOT NULL,
  description TEXT NOT NULL,
  owner_principal TEXT NOT NULL,
  state_version INTEGER NOT NULL DEFAULT 1,
  current_model_snapshot_id TEXT,
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL,
  UNIQUE (organization_id, product_key)
);

CREATE INDEX idx_products_organization
  ON products(organization_id, display_name);

CREATE TABLE repositories (
  id TEXT PRIMARY KEY,
  provider TEXT NOT NULL,
  external_id TEXT NOT NULL,
  canonical_url TEXT NOT NULL,
  default_branch TEXT NOT NULL,
  registered_commit TEXT NOT NULL,
  state_version INTEGER NOT NULL DEFAULT 1,
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL,
  UNIQUE (provider, external_id),
  UNIQUE (canonical_url)
);

CREATE TABLE services (
  id TEXT PRIMARY KEY,
  product_id TEXT NOT NULL REFERENCES products(id),
  service_key TEXT NOT NULL,
  display_name TEXT NOT NULL,
  description TEXT NOT NULL,
  status TEXT NOT NULL,
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL,
  UNIQUE (product_id, service_key)
);

CREATE TABLE repository_bindings (
  id TEXT PRIMARY KEY,
  product_id TEXT NOT NULL REFERENCES products(id),
  repository_id TEXT NOT NULL REFERENCES repositories(id),
  status TEXT NOT NULL,
  current_revision_id TEXT,
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL,
  UNIQUE (product_id, repository_id)
);

CREATE INDEX idx_repository_bindings_repository
  ON repository_bindings(repository_id, product_id);

CREATE TABLE repository_binding_revisions (
  id TEXT PRIMARY KEY,
  binding_id TEXT NOT NULL REFERENCES repository_bindings(id),
  revision INTEGER NOT NULL,
  service_ids_json TEXT NOT NULL,
  scopes_json TEXT NOT NULL,
  status TEXT NOT NULL,
  evidence_json TEXT NOT NULL,
  content_hash TEXT NOT NULL,
  reviewed_by TEXT NOT NULL,
  review_reason TEXT NOT NULL,
  created_at TEXT NOT NULL,
  UNIQUE (binding_id, revision)
);

CREATE TABLE product_model_snapshots (
  id TEXT PRIMARY KEY,
  product_id TEXT NOT NULL REFERENCES products(id),
  version INTEGER NOT NULL,
  model_json TEXT NOT NULL,
  content_hash TEXT NOT NULL,
  created_by TEXT NOT NULL,
  creation_reason TEXT NOT NULL,
  created_at TEXT NOT NULL,
  UNIQUE (product_id, version),
  UNIQUE (product_id, content_hash)
);

CREATE INDEX idx_product_model_snapshots_product
  ON product_model_snapshots(product_id, version DESC);

ALTER TABLE work_items ADD COLUMN mode TEXT;
ALTER TABLE work_items ADD COLUMN product_id TEXT REFERENCES products(id);
ALTER TABLE work_items ADD COLUMN mutable_repository_id TEXT REFERENCES repositories(id);
ALTER TABLE work_items ADD COLUMN product_model_snapshot_id TEXT REFERENCES product_model_snapshots(id);
ALTER TABLE work_items ADD COLUMN product_model_snapshot_hash TEXT;
ALTER TABLE work_items ADD COLUMN repository_contract_version_id TEXT;
ALTER TABLE work_items ADD COLUMN selected_acceptance_names_json TEXT NOT NULL DEFAULT '[]';
ALTER TABLE work_items ADD COLUMN context_repository_snapshots_json TEXT NOT NULL DEFAULT '[]';
ALTER TABLE work_items ADD COLUMN current_stage_execution_id TEXT;
ALTER TABLE work_items ADD COLUMN state_version INTEGER NOT NULL DEFAULT 1;
ALTER TABLE work_items ADD COLUMN closed_at TEXT;
ALTER TABLE work_items ADD COLUMN closure_reason TEXT;

CREATE INDEX idx_work_items_product_status
  ON work_items(product_id, status, created_at DESC);
