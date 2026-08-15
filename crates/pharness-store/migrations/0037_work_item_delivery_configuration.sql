ALTER TABLE work_items ADD COLUMN source_commit TEXT;
ALTER TABLE work_items ADD COLUMN pipeline_contract_id TEXT;
ALTER TABLE work_items ADD COLUMN deployment_contract_id TEXT;
ALTER TABLE work_items ADD COLUMN gitops_kustomization_path TEXT;
ALTER TABLE work_items ADD COLUMN gitops_image_name TEXT;
ALTER TABLE work_items ADD COLUMN workload_kind TEXT;
ALTER TABLE work_items ADD COLUMN workload_name TEXT;
ALTER TABLE work_items ADD COLUMN rollback_owner TEXT;

