CREATE TABLE repository_onboardings (
  id TEXT PRIMARY KEY,
  product_id TEXT NOT NULL REFERENCES products(id),
  repository_id TEXT NOT NULL REFERENCES repositories(id),
  binding_id TEXT NOT NULL REFERENCES repository_bindings(id),
  onboarding_kind TEXT NOT NULL,
  status TEXT NOT NULL,
  registered_commit TEXT NOT NULL,
  resolved_commit TEXT,
  current_discovery_id TEXT,
  current_proposal_revision INTEGER NOT NULL DEFAULT 0,
  approved_proposal_hash TEXT,
  source_delivery_intent_id TEXT,
  contract_version_id TEXT,
  state_version INTEGER NOT NULL DEFAULT 1,
  blockers_json TEXT NOT NULL DEFAULT '[]',
  created_by TEXT NOT NULL,
  creation_reason TEXT NOT NULL,
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL,
  status_changed_at TEXT NOT NULL,
  status_changed_by TEXT,
  status_reason TEXT
);

CREATE INDEX idx_repository_onboardings_repository
  ON repository_onboardings(repository_id, created_at DESC);
CREATE INDEX idx_repository_onboardings_product
  ON repository_onboardings(product_id, status, created_at DESC);

CREATE TABLE repository_discoveries (
  id TEXT PRIMARY KEY,
  onboarding_id TEXT NOT NULL REFERENCES repository_onboardings(id),
  source_commit TEXT NOT NULL,
  resolved_commit TEXT,
  status TEXT NOT NULL,
  schema_version TEXT NOT NULL,
  inventory_json TEXT,
  content_hash TEXT,
  error_code TEXT,
  error_summary TEXT,
  started_at TEXT,
  finished_at TEXT,
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL
);

CREATE INDEX idx_repository_discoveries_onboarding
  ON repository_discoveries(onboarding_id, created_at DESC);

CREATE TABLE repository_onboarding_proposals (
  id TEXT PRIMARY KEY,
  onboarding_id TEXT NOT NULL REFERENCES repository_onboardings(id),
  revision INTEGER NOT NULL,
  status TEXT NOT NULL,
  proposal_json TEXT NOT NULL,
  content_hash TEXT NOT NULL,
  discovery_id TEXT NOT NULL REFERENCES repository_discoveries(id),
  discovery_hash TEXT NOT NULL,
  created_by TEXT NOT NULL,
  origin TEXT NOT NULL,
  created_at TEXT NOT NULL,
  UNIQUE (onboarding_id, revision)
);

CREATE TABLE repository_contract_versions (
  id TEXT PRIMARY KEY,
  repository_id TEXT NOT NULL REFERENCES repositories(id),
  onboarding_id TEXT NOT NULL REFERENCES repository_onboardings(id),
  source_commit TEXT NOT NULL,
  contract_path TEXT NOT NULL,
  api_version TEXT NOT NULL,
  contract_json TEXT NOT NULL,
  content_hash TEXT NOT NULL,
  merge_provenance_json TEXT NOT NULL,
  status TEXT NOT NULL,
  created_at TEXT NOT NULL,
  UNIQUE (repository_id, source_commit, content_hash)
);

CREATE INDEX idx_repository_contract_versions_repository
  ON repository_contract_versions(repository_id, created_at DESC);

CREATE TABLE repository_readiness_assessments (
  id TEXT PRIMARY KEY,
  repository_id TEXT NOT NULL REFERENCES repositories(id),
  source_commit TEXT NOT NULL,
  contract_version_id TEXT REFERENCES repository_contract_versions(id),
  contract_hash TEXT,
  dependency_lock_hash TEXT,
  environment_profile_id TEXT,
  environment_profile_revision TEXT,
  runner_image_digest TEXT,
  validation_policy_version TEXT NOT NULL,
  contract_status TEXT NOT NULL,
  coding_status TEXT NOT NULL,
  checks_json TEXT NOT NULL,
  blockers_json TEXT NOT NULL,
  warnings_json TEXT NOT NULL,
  evidence_refs_json TEXT NOT NULL,
  input_hash TEXT NOT NULL,
  content_hash TEXT NOT NULL,
  assessed_at TEXT NOT NULL,
  expires_at TEXT,
  UNIQUE (repository_id, input_hash)
);

CREATE INDEX idx_repository_readiness_revision
  ON repository_readiness_assessments(repository_id, source_commit, assessed_at DESC);

CREATE TABLE source_delivery_intents (
  id TEXT PRIMARY KEY,
  subject_kind TEXT NOT NULL,
  subject_id TEXT NOT NULL,
  repository_id TEXT NOT NULL REFERENCES repositories(id),
  source_repo TEXT NOT NULL,
  base_ref TEXT NOT NULL,
  base_commit TEXT NOT NULL,
  head_branch TEXT NOT NULL,
  patch_artifact_id TEXT,
  patch_hash TEXT NOT NULL,
  status TEXT NOT NULL,
  state_version INTEGER NOT NULL DEFAULT 1,
  authorization_json TEXT,
  writer_execution_id TEXT,
  observer_execution_id TEXT,
  pull_request_json TEXT,
  merge_provenance_json TEXT,
  provider_checks_json TEXT,
  created_by TEXT NOT NULL,
  creation_reason TEXT NOT NULL,
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL,
  status_changed_at TEXT NOT NULL,
  status_changed_by TEXT,
  status_reason TEXT,
  UNIQUE (subject_kind, subject_id)
);

CREATE INDEX idx_source_delivery_intents_repository
  ON source_delivery_intents(repository_id, status, created_at DESC);
