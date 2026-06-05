ALTER TABLE observations ADD COLUMN resource_namespace TEXT;
ALTER TABLE observations ADD COLUMN resource_kind TEXT;
ALTER TABLE observations ADD COLUMN resource_name TEXT;

CREATE INDEX idx_observations_resource_identity
  ON observations(resource_namespace, resource_kind, resource_name, observed_at DESC);
