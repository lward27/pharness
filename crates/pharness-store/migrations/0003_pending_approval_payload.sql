ALTER TABLE approvals ADD COLUMN action_json TEXT;
ALTER TABLE approvals ADD COLUMN resume_messages_json TEXT;
ALTER TABLE approvals ADD COLUMN turns_completed INTEGER NOT NULL DEFAULT 0;
