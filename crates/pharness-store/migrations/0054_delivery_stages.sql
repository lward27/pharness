-- Extend one immutable build to independent staging and production deliveries.
-- Existing rows keep their original, explicitly legacy contract. No workflow
-- writes are enabled by this migration.
ALTER TABLE deployment_intents ADD COLUMN delivery_stage TEXT NOT NULL DEFAULT 'legacy'
  CHECK (delivery_stage IN ('legacy', 'staging', 'production'));
DROP INDEX idx_deployment_intents_pipeline_intent;
CREATE UNIQUE INDEX idx_deployment_intents_pipeline_stage
  ON deployment_intents(pipeline_intent_id, delivery_stage);

-- SQLite cannot remove the embedded UNIQUE(pipeline_intent_id) constraint.
-- SQLx runs this data-preserving replacement atomically on the existing
-- foreign-keys-disabled migration connection; the normal pool restores FKs.
CREATE TABLE gitops_change_sets_v2 (
  id TEXT PRIMARY KEY,
  work_item_id TEXT NOT NULL REFERENCES work_items(id),
  work_plan_id TEXT NOT NULL REFERENCES work_plans(id),
  source_change_set_id TEXT NOT NULL REFERENCES change_sets(id),
  pipeline_intent_id TEXT NOT NULL REFERENCES pipeline_intents(id),
  deployment_intent_id TEXT NOT NULL REFERENCES deployment_intents(id),
  gitops_update_plan_artifact_id TEXT NOT NULL REFERENCES artifacts(id),
  session_id TEXT NOT NULL REFERENCES sessions(id),
  run_id TEXT REFERENCES runs(id),
  status TEXT NOT NULL,
  title TEXT NOT NULL,
  summary TEXT NOT NULL,
  risk_level TEXT NOT NULL,
  material_hash TEXT NOT NULL,
  revision INTEGER NOT NULL DEFAULT 1,
  gitops_repo TEXT NOT NULL,
  gitops_ref TEXT NOT NULL,
  head_branch TEXT NOT NULL,
  kustomization_path TEXT NOT NULL,
  image_name TEXT NOT NULL,
  image_ref TEXT NOT NULL,
  gitops_change_set_json TEXT NOT NULL DEFAULT '{}',
  created_at TEXT NOT NULL,
  updated_at TEXT,
  status_changed_at TEXT,
  status_changed_by TEXT,
  status_reason TEXT,
  UNIQUE(deployment_intent_id)
);
INSERT INTO gitops_change_sets_v2 SELECT * FROM gitops_change_sets;
DROP TABLE gitops_change_sets;
ALTER TABLE gitops_change_sets_v2 RENAME TO gitops_change_sets;
CREATE INDEX idx_gitops_change_sets_work_item
  ON gitops_change_sets(work_item_id, created_at DESC);
CREATE INDEX idx_gitops_change_sets_pipeline_intent
  ON gitops_change_sets(pipeline_intent_id, created_at DESC);
CREATE INDEX idx_gitops_change_sets_deployment_intent
  ON gitops_change_sets(deployment_intent_id, created_at DESC);
CREATE INDEX idx_gitops_change_sets_status_created
  ON gitops_change_sets(status, created_at DESC);

-- A legacy binary must not create a legacy deployment for hosted work.
CREATE TRIGGER deployment_stage_matches_workflow
BEFORE INSERT ON deployment_intents
WHEN (NEW.delivery_stage = 'legacy') != NOT EXISTS (
  SELECT 1 FROM pipeline_intents p JOIN work_plans w ON w.id = p.work_plan_id
    JOIN work_items i ON i.id = w.work_item_id
  WHERE p.id = NEW.pipeline_intent_id AND i.workflow_policy_json IS NOT NULL
)
BEGIN
  SELECT RAISE(ABORT, 'deployment stage does not match the recorded workflow');
END;

CREATE TRIGGER deployment_stage_identity_immutable
BEFORE UPDATE ON deployment_intents
WHEN OLD.delivery_stage IS NOT NEW.delivery_stage
  OR OLD.pipeline_intent_id IS NOT NEW.pipeline_intent_id
  OR (OLD.delivery_stage != 'legacy' AND (
    OLD.work_plan_id IS NOT NEW.work_plan_id OR OLD.change_set_id IS NOT NEW.change_set_id
    OR OLD.session_id IS NOT NEW.session_id OR OLD.run_id IS NOT NEW.run_id
    OR OLD.target_environment IS NOT NEW.target_environment
    OR OLD.target_namespace IS NOT NEW.target_namespace
    OR OLD.argo_application IS NOT NEW.argo_application
  ))
BEGIN
  SELECT RAISE(ABORT, 'deployment stage and build identity are immutable');
END;

CREATE TRIGGER hosted_deployment_lineage
BEFORE INSERT ON deployment_intents
WHEN NEW.delivery_stage != 'legacy' AND (
  NEW.target_environment IS NOT NEW.delivery_stage
  OR NEW.target_namespace IS NULL OR NEW.argo_application IS NULL
  OR trim(NEW.target_namespace) = '' OR trim(NEW.argo_application) = ''
  OR NOT EXISTS (SELECT 1 FROM pipeline_intents p WHERE p.id = NEW.pipeline_intent_id
    AND p.change_set_id = NEW.change_set_id AND p.work_plan_id = NEW.work_plan_id
    AND p.session_id = NEW.session_id AND p.run_id IS NEW.run_id)
)
BEGIN
  SELECT RAISE(ABORT, 'hosted deployment requires its original build lineage and explicit target');
END;

CREATE TRIGGER hosted_gitops_deployment_lineage
BEFORE INSERT ON gitops_change_sets
WHEN EXISTS (SELECT 1 FROM deployment_intents d WHERE d.id = NEW.deployment_intent_id
  AND d.delivery_stage != 'legacy') AND NOT EXISTS (
  SELECT 1 FROM deployment_intents d JOIN work_plans w ON w.id = d.work_plan_id
  WHERE d.id = NEW.deployment_intent_id AND d.pipeline_intent_id = NEW.pipeline_intent_id
    AND d.work_plan_id = NEW.work_plan_id AND d.change_set_id = NEW.source_change_set_id
    AND d.session_id = NEW.session_id AND d.run_id IS NEW.run_id
    AND w.work_item_id = NEW.work_item_id
)
BEGIN
  SELECT RAISE(ABORT, 'GitOps change must belong to its hosted deployment and build');
END;

CREATE TRIGGER legacy_gitops_requires_run
BEFORE INSERT ON gitops_change_sets
WHEN NEW.run_id IS NULL AND NOT EXISTS (
  SELECT 1 FROM deployment_intents d WHERE d.id = NEW.deployment_intent_id
    AND d.delivery_stage != 'legacy'
)
BEGIN
  SELECT RAISE(ABORT, 'legacy GitOps changes require coding-run provenance');
END;

CREATE TRIGGER gitops_deployment_identity_immutable
BEFORE UPDATE ON gitops_change_sets
WHEN OLD.pipeline_intent_id IS NOT NEW.pipeline_intent_id
  OR OLD.deployment_intent_id IS NOT NEW.deployment_intent_id
  OR OLD.work_item_id IS NOT NEW.work_item_id OR OLD.work_plan_id IS NOT NEW.work_plan_id
  OR OLD.source_change_set_id IS NOT NEW.source_change_set_id
  OR OLD.session_id IS NOT NEW.session_id OR OLD.run_id IS NOT NEW.run_id
BEGIN
  SELECT RAISE(ABORT, 'GitOps deployment and source identity are immutable');
END;

-- Fail the entire migration if the preserved graph contains dangling links.
CREATE TABLE delivery_migration_fk_guard (violations INTEGER CHECK (violations = 0));
INSERT INTO delivery_migration_fk_guard SELECT COUNT(*) FROM pragma_foreign_key_check;
DROP TABLE delivery_migration_fk_guard;
