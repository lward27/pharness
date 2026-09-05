use super::{RepositoryOnboardingProposal, ONBOARDING_PROPOSAL_SCHEMA};
use crate::RepositoryContract;

const LEGACY_SCHEMA: &str = "pharness.dev/repository-onboarding-proposal/v1alpha1";

impl RepositoryOnboardingProposal {
    pub fn is_blocked(&self) -> bool {
        !self.blockers.is_empty() || !self.conflicts.is_empty()
    }

    /// A retained proposal is evidence, not authority to write or execute its contract.
    /// V1 remains readable; only V2 can represent an explicitly absent candidate.
    pub fn validate_submission(&self) -> Result<Option<RepositoryContract>, String> {
        if serde_json::to_vec(self)
            .map_err(|error| error.to_string())?
            .len()
            > 128 * 1024
        {
            return Err("onboarding proposal exceeds the 128 KiB submission limit".into());
        }
        if !matches!(
            self.schema_version.as_str(),
            ONBOARDING_PROPOSAL_SCHEMA | LEGACY_SCHEMA
        ) {
            return Err("unsupported onboarding proposal schema_version".into());
        }
        if self.discovery_id.trim().is_empty()
            || !self
                .discovery_hash
                .strip_prefix("sha256:")
                .is_some_and(|hash| {
                    hash.len() == 64
                        && hash
                            .bytes()
                            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
                })
        {
            return Err("onboarding proposal requires exact discovery identity and SHA-256".into());
        }
        if self.instructions.len() > 32 * 1024
            || self.service_proposals.len() > 50
            || self.binding_proposals.len() > 50
            || !self.readiness_forecast.is_object()
        {
            return Err("onboarding proposal exceeds its bounded schema".into());
        }
        for (name, values) in [
            ("blockers", &self.blockers),
            ("conflicts", &self.conflicts),
            ("assumptions", &self.assumptions),
        ] {
            if values.len() > 100 || values.iter().any(|s| s.trim().is_empty() || s.len() > 2000) {
                return Err(format!(
                    "{name} must contain at most 100 nonempty statements of at most 2000 bytes"
                ));
            }
        }
        if self.candidate_contract.is_null() {
            if self.schema_version == ONBOARDING_PROPOSAL_SCHEMA && self.is_blocked() {
                return Ok(None);
            }
            return Err("candidate_contract may be null only in a v1alpha2 proposal with an explicit blocker or conflict".into());
        }
        let contract: RepositoryContract = serde_json::from_value(self.candidate_contract.clone())
            .map_err(|error| format!("candidate repository contract is invalid: {error}"))?;
        contract
            .validate_candidate()
            .map_err(|error| error.to_string())?;
        Ok(Some(contract))
    }

    /// Recheck at each approval/materialization boundary, including historical proposals.
    pub fn approvable_contract(&self) -> Result<RepositoryContract, String> {
        let candidate = self.validate_submission()?;
        if self.is_blocked() {
            return Err("onboarding proposal has unresolved blockers or conflicts; revise the proposal or refresh its source evidence before approval".into());
        }
        candidate.ok_or_else(|| "onboarding proposal has no executable candidate contract".into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn proposal() -> RepositoryOnboardingProposal {
        serde_json::from_value(json!({
            "schema_version":ONBOARDING_PROPOSAL_SCHEMA,
            "discovery_id":"rdisc_test","discovery_hash":format!("sha256:{}", "a".repeat(64)),
            "candidate_contract":null,"instructions":"Missing lock must be supplied in source.",
            "blockers":["immutable_dependency_lock_missing"],"readiness_forecast":{}
        }))
        .unwrap()
    }

    #[test]
    fn retains_explicit_absence_without_granting_approval() {
        let mut proposal = proposal();
        assert!(proposal.validate_submission().unwrap().is_none());
        assert!(proposal.approvable_contract().is_err());
        proposal.blockers.clear();
        assert!(proposal.validate_submission().is_err());
        proposal
            .conflicts
            .push("conflicting contract aliases".into());
        assert!(proposal.validate_submission().unwrap().is_none());
        proposal.conflicts = vec![" ".into()];
        assert!(proposal.validate_submission().is_err());
        proposal.conflicts = vec!["unresolved".into()];
        proposal.readiness_forecast = json!({"unsupported_detail":"x".repeat(128 * 1024)});
        assert!(proposal
            .validate_submission()
            .unwrap_err()
            .contains("128 KiB"));
    }

    #[test]
    fn legacy_candidates_remain_readable_but_contradictions_never_authorize() {
        let mut proposal = proposal();
        proposal.schema_version = LEGACY_SCHEMA.into();
        assert!(proposal.validate_submission().is_err());
        proposal.candidate_contract = json!({
            "api_version":"pharness.dev/v1alpha1","environment_profile":"python-3.11",
            "dependency_lock":{"kind":"pip_requirements","path":"requirements.lock","sha256":"b".repeat(64)},
            "writable_paths":["src/**","tests/**"],
            "acceptance_commands":[{"name":"unit","command":"python -m unittest discover -s tests -v"}],
            "roots":{"source":["src"],"tests":["tests"],"documentation":[]},
            "agent_network":"denied","package_installation":"preparation_only"
        });
        assert!(proposal.validate_submission().unwrap().is_some());
        assert!(proposal.approvable_contract().is_err());
        proposal.blockers.clear();
        assert!(proposal.approvable_contract().is_ok());
        proposal.conflicts.push("alias differs".into());
        assert!(proposal.approvable_contract().is_err());
        proposal.candidate_contract["dependency_lock"]["sha256"] = json!("invented");
        assert!(proposal.validate_submission().is_err());
    }
}
