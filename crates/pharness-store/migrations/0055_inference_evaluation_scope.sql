-- Existing and legacy-created evaluations retain their original full-suite contract.
ALTER TABLE inference_evaluations ADD COLUMN scope_json TEXT NOT NULL
    DEFAULT '{"kind":"full_qualification"}';
