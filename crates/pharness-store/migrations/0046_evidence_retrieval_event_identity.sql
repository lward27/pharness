ALTER TABLE evidence_retrievals ADD COLUMN event_id TEXT;

CREATE UNIQUE INDEX idx_evidence_retrievals_event
  ON evidence_retrievals(event_id)
  WHERE event_id IS NOT NULL;
