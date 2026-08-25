ALTER TABLE work_items ADD COLUMN contract_version TEXT;

ALTER TABLE repository_onboardings ADD COLUMN proposer_run_id TEXT REFERENCES runs(id);
ALTER TABLE repository_onboardings ADD COLUMN proposer_profile_hash TEXT;
ALTER TABLE repository_onboardings ADD COLUMN proposer_stop_reason TEXT;
ALTER TABLE repository_onboardings ADD COLUMN patch_execution_id TEXT;
ALTER TABLE repository_onboardings ADD COLUMN patch_artifact_id TEXT;
ALTER TABLE repository_onboardings ADD COLUMN patch_hash TEXT;

CREATE UNIQUE INDEX idx_repository_onboardings_proposer_run
  ON repository_onboardings(proposer_run_id)
  WHERE proposer_run_id IS NOT NULL;
