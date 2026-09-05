-- Existing rows remain source-only under their original contract. This is an
-- additive reader migration; hosted writes are enabled separately in config.
ALTER TABLE work_items ADD COLUMN workflow_policy_json TEXT
  CHECK (workflow_policy_json IS NULL OR json_valid(workflow_policy_json));
ALTER TABLE work_items ADD COLUMN workflow_policy_hash TEXT;

CREATE TRIGGER hosted_workflow_policy_insert
BEFORE INSERT ON work_items
WHEN (NEW.workflow_policy_json IS NULL) != (NEW.workflow_policy_hash IS NULL)
 OR (NEW.workflow_policy_json IS NOT NULL AND (
   NEW.mode IS NOT 'repo'
   OR json_extract(NEW.workflow_policy_json, '$.schema_version') IS NOT 'pharness.dev/hosted-workflow/v1alpha1'
   OR NEW.status = 'completed'
 ))
BEGIN
  SELECT RAISE(ABORT, 'invalid hosted workflow policy or premature completion');
END;

CREATE TRIGGER hosted_workflow_policy_immutable
BEFORE UPDATE OF workflow_policy_json, workflow_policy_hash ON work_items
WHEN OLD.workflow_policy_json IS NOT NEW.workflow_policy_json
 OR OLD.workflow_policy_hash IS NOT NEW.workflow_policy_hash
BEGIN
  SELECT RAISE(ABORT, 'workflow policy snapshots are immutable');
END;

CREATE TRIGGER hosted_workflow_scope_immutable
BEFORE UPDATE ON work_items
WHEN OLD.workflow_policy_json IS NOT NULL AND (
 OLD.mode IS NOT NEW.mode OR OLD.product_id IS NOT NEW.product_id
 OR OLD.mutable_repository_id IS NOT NEW.mutable_repository_id
 OR OLD.source_repo IS NOT NEW.source_repo OR OLD.source_ref IS NOT NEW.source_ref
 OR OLD.source_commit IS NOT NEW.source_commit
 OR OLD.product_model_snapshot_id IS NOT NEW.product_model_snapshot_id
 OR OLD.product_model_snapshot_hash IS NOT NEW.product_model_snapshot_hash
 OR OLD.repository_contract_version_id IS NOT NEW.repository_contract_version_id
 OR OLD.repository_contract_json IS NOT NEW.repository_contract_json
 OR OLD.repository_contract_hash IS NOT NEW.repository_contract_hash
 OR OLD.context_repository_snapshots_json IS NOT NEW.context_repository_snapshots_json
 OR OLD.selected_acceptance_names_json IS NOT NEW.selected_acceptance_names_json
 OR OLD.acceptance_criteria_json IS NOT NEW.acceptance_criteria_json
 OR OLD.run_budget_json IS NOT NEW.run_budget_json
 OR OLD.max_attempts IS NOT NEW.max_attempts
 OR OLD.max_elapsed_seconds IS NOT NEW.max_elapsed_seconds
 OR OLD.production_impacting IS NOT NEW.production_impacting
)
BEGIN
  SELECT RAISE(ABORT, 'hosted scope and execution limits are immutable');
END;

-- These guards also stop an older binary from converting a hosted WorkItem
-- into a successful source-only result after an unsafe application rollback.
CREATE TRIGGER hosted_workflow_completion_requires_all_stages
BEFORE UPDATE OF status ON work_items
WHEN NEW.workflow_policy_json IS NOT NULL AND NEW.status = 'completed'
 AND (SELECT COUNT(*)
      FROM effective_stage_outcomes e JOIN stage_outcomes o ON o.id = e.outcome_id
      WHERE e.work_item_id = NEW.id AND o.work_item_id = NEW.id
        AND e.stage_key = o.stage_key AND o.status = 'succeeded'
        AND e.stage_key IN ('discover', 'plan', 'implement', 'test', 'verify',
                            'source_delivery', 'release', 'observe')) != 8
BEGIN
  SELECT RAISE(ABORT, 'hosted completion requires successful source, release, and runtime evidence');
END;

CREATE TRIGGER hosted_release_stage_cannot_be_inapplicable
BEFORE INSERT ON stage_executions
WHEN NEW.stage_key IN ('release', 'observe') AND NEW.status = 'inapplicable'
 AND EXISTS (SELECT 1 FROM work_items WHERE id = NEW.work_item_id AND workflow_policy_json IS NOT NULL)
BEGIN
  SELECT RAISE(ABORT, 'hosted release and observation are required');
END;

CREATE TRIGGER hosted_release_outcome_cannot_be_inapplicable
BEFORE INSERT ON stage_outcomes
WHEN NEW.stage_key IN ('release', 'observe') AND NEW.status = 'inapplicable'
 AND EXISTS (SELECT 1 FROM work_items WHERE id = NEW.work_item_id AND workflow_policy_json IS NOT NULL)
BEGIN
  SELECT RAISE(ABORT, 'hosted release and observation are required');
END;
