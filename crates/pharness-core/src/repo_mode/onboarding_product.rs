use super::RepositoryOnboardingProposal;
use std::collections::BTreeSet;

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum RepositoryProductProposalError {
    #[error("{0}")]
    Invalid(String),
    #[error("{0}")]
    Conflict(String),
}

/// The same deterministic checks run against a Run's original product snapshot
/// at submission and against current product state before approval/materialization.
impl RepositoryOnboardingProposal {
    pub fn validate_product_proposals<'a>(
        &self,
        existing_service_keys: impl IntoIterator<Item = &'a str>,
    ) -> Result<(), RepositoryProductProposalError> {
        use RepositoryProductProposalError::{Conflict, Invalid};
        if self.service_proposals.len() > 32 {
            return Err(Invalid(
                "service_proposals may define at most 32 new Services".into(),
            ));
        }
        if self.binding_proposals.len() > 1 {
            return Err(Invalid(
                "binding_proposals permits at most one binding for this Repository".into(),
            ));
        }
        let existing = existing_service_keys.into_iter().collect::<BTreeSet<_>>();
        let mut service_keys = existing.clone();
        for (index, service) in self.service_proposals.iter().enumerate() {
            let field = format!("service_proposals[{index}]");
            let key = service.service_key.as_str();
            if key.len() > 64
                || key.split('-').any(|part| {
                    part.is_empty()
                        || !part
                            .bytes()
                            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit())
                })
            {
                return Err(Invalid(format!("{field}.service_key must be a canonical 1-64 character lowercase alphanumeric key with single hyphen separators")));
            }
            let display_name = service.display_name.trim();
            if display_name.is_empty() || display_name.len() > 120 {
                return Err(Invalid(format!(
                    "{field}.display_name must be between 1 and 120 characters"
                )));
            }
            if service.description.len() > 4_000 {
                return Err(Invalid(format!(
                    "{field}.description exceeds 4,000 characters"
                )));
            }
            if existing.contains(key) {
                return Err(Conflict(format!("{field}.service_key {key:?} already exists in the Product; service_proposals creates new Services. Reuse the existing key in binding_proposals.service_keys without proposing it again")));
            }
            if !service_keys.insert(key) {
                return Err(Invalid(format!(
                    "{field}.service_key repeats Service {key:?}"
                )));
            }
        }
        if let Some(binding) = self.binding_proposals.first() {
            if binding.scopes.is_empty() || binding.scopes.len() > 64 {
                return Err(Invalid("binding_proposals[0].scopes must contain between one and 64 repository-relative globs".into()));
            }
            let mut seen_services = BTreeSet::new();
            for (index, key) in binding.service_keys.iter().enumerate() {
                let field = format!("binding_proposals[0].service_keys[{index}]");
                if !seen_services.insert(key) {
                    return Err(Invalid(format!("{field} repeats Service key {key:?}")));
                }
                if !service_keys.contains(key.as_str()) {
                    return Err(Invalid(format!("{field} references unknown Service {key:?}; use an existing Product key or a separately proposed new Service")));
                }
            }
            let mut seen_scopes = BTreeSet::new();
            for (index, scope) in binding.scopes.iter().enumerate() {
                validate_repository_binding_scope(scope).map_err(|message| {
                    Invalid(format!("binding_proposals[0].scopes[{index}]: {message}"))
                })?;
                if !seen_scopes.insert(scope) {
                    return Err(Invalid(format!(
                        "binding_proposals[0].scopes[{index}] repeats scope {scope:?}"
                    )));
                }
            }
        }
        Ok(())
    }
}

pub fn validate_repository_binding_scope(scope: &str) -> Result<(), String> {
    if scope.is_empty()
        || scope.len() > 256
        || scope.starts_with(['/', '~'])
        || scope.contains(['\\', '\n', '\r', '\0'])
        || scope
            .split('/')
            .any(|segment| segment.is_empty() || matches!(segment, "." | ".."))
    {
        return Err(format!(
            "binding scope {scope:?} is not a normalized repository-relative glob"
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn proposal() -> RepositoryOnboardingProposal {
        serde_json::from_value(json!({
            "schema_version":super::super::ONBOARDING_PROPOSAL_SCHEMA,
            "discovery_id":"original", "discovery_hash":format!("sha256:{}", "a".repeat(64)),
            "candidate_contract":null, "instructions":"Resolve the missing lock.",
            "blockers":["immutable_dependency_lock_missing"], "readiness_forecast":{},
            "service_proposals":[{"service_key":"new-api", "display_name":"New API", "description":"Requested component"}],
            "binding_proposals":[{"service_keys":["existing-web", "new-api"], "scopes":["src/**"]}]
        })).unwrap()
    }

    #[test]
    fn creation_and_reuse_are_distinct_and_current_state_is_rechecked() {
        let proposal = proposal();
        assert!(proposal
            .validate_product_proposals(["existing-web"])
            .is_ok());
        // A concurrent creation changes approval eligibility, not the old proposal.
        assert!(
            matches!(proposal.validate_product_proposals(["existing-web", "new-api"]),
            Err(RepositoryProductProposalError::Conflict(message)) if message.contains("binding_proposals"))
        );
        let mut reuse = proposal.clone();
        reuse.service_proposals.clear();
        reuse.binding_proposals[0].service_keys = vec!["existing-web".into()];
        assert!(reuse.validate_product_proposals(["existing-web"]).is_ok());
        assert!(matches!(reuse.validate_product_proposals([]),
            Err(RepositoryProductProposalError::Invalid(message)) if message.contains("unknown Service")));
        reuse.binding_proposals.clear();
        assert!(reuse.validate_product_proposals([]).is_ok());
        assert!(reuse.validate_submission().unwrap().is_none());
        assert!(reuse.approvable_contract().is_err());
    }

    #[test]
    fn invalid_product_fields_fail_before_materialization() {
        let good = serde_json::to_value(proposal()).unwrap();
        for (pointer, value, expected) in [
            (
                "/service_proposals/0/service_key",
                json!("New API"),
                "service_key",
            ),
            (
                "/service_proposals/0/display_name",
                json!(" "),
                "display_name",
            ),
            (
                "/service_proposals/0/description",
                json!("x".repeat(4001)),
                "description",
            ),
            (
                "/service_proposals",
                json!([good["service_proposals"][0], good["service_proposals"][0]]),
                "repeats Service",
            ),
            (
                "/binding_proposals",
                json!([good["binding_proposals"][0], good["binding_proposals"][0]]),
                "at most one",
            ),
            (
                "/binding_proposals/0/service_keys",
                json!(["unknown"]),
                "unknown Service",
            ),
            (
                "/binding_proposals/0/service_keys",
                json!(["existing-web", "existing-web"]),
                "repeats Service",
            ),
            ("/binding_proposals/0/scopes", json!([]), "scopes"),
            (
                "/binding_proposals/0/scopes",
                json!(["src/**", "src/**"]),
                "repeats scope",
            ),
            (
                "/binding_proposals/0/scopes",
                json!(["../secrets"]),
                "normalized repository-relative",
            ),
        ] {
            let mut changed = good.clone();
            *changed.pointer_mut(pointer).unwrap() = value;
            let proposal: RepositoryOnboardingProposal = serde_json::from_value(changed).unwrap();
            let error = proposal
                .validate_product_proposals(["existing-web"])
                .unwrap_err();
            assert!(error.to_string().contains(expected), "{pointer}: {error}");
        }
    }
}
