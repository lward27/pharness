ALTER TABLE repository_onboardings
  ADD COLUMN readiness_assessment_id TEXT REFERENCES repository_readiness_assessments(id);

CREATE INDEX idx_repository_onboardings_readiness
  ON repository_onboardings(readiness_assessment_id);
