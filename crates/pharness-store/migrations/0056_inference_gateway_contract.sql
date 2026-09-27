-- Model qualification binds to the gateway contract and registry-derived
-- policy/target/suite hashes instead of the application release. Rows created
-- before this migration have no contract and never satisfy a current gate.
ALTER TABLE inference_evaluations ADD COLUMN gateway_contract TEXT;
ALTER TABLE inference_policy_qualifications ADD COLUMN gateway_contract TEXT;
