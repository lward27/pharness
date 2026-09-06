//! The model proposes configuration; the original Run supplies discovery identity.
//! Absence of a saved contract means legacy submission, including on resume.
use crate::{prompt::StagePromptPack, RunSpec};
use pharness_core::{RepositoryOnboardingProposal, ResolvedInferenceBinding, ToolError};
use serde_json::{json, Value};

pub const ONBOARDING_SUBMISSION_CONTRACT: &str = "pharness.dev/onboarding-agent-submission/v1";
const IDENTITY_FIELDS: [&str; 3] = ["schema_version", "discovery_id", "discovery_hash"];
const PROPOSAL_FIELDS: [&str; 8] = [
    "candidate_contract",
    "instructions",
    "service_proposals",
    "binding_proposals",
    "assumptions",
    "conflicts",
    "blockers",
    "readiness_forecast",
];

pub(super) fn current_prompt() -> StagePromptPack {
    StagePromptPack {
        prompt_id: "repo-onboarding-v2",
        revision: "2026-09-06.2",
        stage: pharness_core::InferenceStage::Onboarding,
        content: r#"Use discovery evidence to propose repository configuration. Separate discovered facts, proposed configuration, assumptions, conflicts and blockers. Copy executable facts only from discovery and compatible profile descriptors; do not invent roots, commands, locks, Services or profile IDs. Submit only the configuration and explanation fields exposed by submit_onboarding_proposal. The controller attaches schema_version, discovery_id and discovery_hash from this Run; omit those fields. If required facts are absent or contradictory, submit candidate_contract: null with explicit blockers or conflicts. A blocked proposal is retained evidence and cannot authorize source changes."#,
    }
}

/// Choose from the saved selection, never from a newly compiled default profile.
/// The worker independently checks the complete tool hash before any model request.
pub fn onboarding_submission_contract_for_binding(
    binding: &ResolvedInferenceBinding,
) -> Option<&'static str> {
    (binding.stage_prompt.as_ref() == Some(&current_prompt().revision_record()))
        .then_some(ONBOARDING_SUBMISSION_CONTRACT)
}

pub(super) fn uses_controller_binding(run: &RunSpec) -> anyhow::Result<bool> {
    let Some(contract) = run
        .execution_target_json
        .get("onboarding_submission_contract")
        .filter(|value| !value.is_null())
    else {
        return Ok(false);
    };
    if contract.as_str() != Some(ONBOARDING_SUBMISSION_CONTRACT) {
        anyhow::bail!("unsupported onboarding_submission_contract");
    }
    if crate::reliability_v2_profile_id(run) != Some("repository-onboarding-proposer") {
        anyhow::bail!("onboarding_submission_contract requires the onboarding V2 profile");
    }
    Ok(true)
}

pub(super) fn constrain_schema(schema: &mut Value) {
    let proposal = &mut schema["properties"]["proposal"];
    proposal["required"] = json!(PROPOSAL_FIELDS);
    proposal["description"] = json!("Proposed configuration and explanation only. The controller attaches schema_version, discovery_id and discovery_hash from the original Run; do not supply those fields.");
    let properties = proposal["properties"]
        .as_object_mut()
        .expect("compiled proposal schema");
    for field in IDENTITY_FIELDS {
        properties.remove(field);
    }
}

#[derive(Debug, Clone)]
pub(super) struct OnboardingBinding {
    controller_bound: bool,
    discovery_id: String,
    discovery_hash: String,
}

impl OnboardingBinding {
    pub(super) fn for_run(run: &RunSpec) -> anyhow::Result<Option<Self>> {
        let controller_bound = uses_controller_binding(run)?;
        let context = &run.execution_target_json["agent_context"];
        let discovery = &context["discovery"];
        let identity = discovery["id"].as_str().zip(discovery["hash"].as_str());
        let Some((id, hash)) = identity else {
            if controller_bound {
                anyhow::bail!("agent_context.discovery requires original id and hash");
            }
            return Ok(None);
        };
        if controller_bound {
            let original = &run.execution_target_json["onboarding"];
            if context["schema_version"] != pharness_core::AGENT_CONTEXT_SCHEMA {
                anyhow::bail!("onboarding submission requires the saved AgentContext schema");
            }
            for (field, expected) in [("discovery_id", id), ("discovery_hash", hash)] {
                if original[field].as_str() != Some(expected) {
                    anyhow::bail!("onboarding.{field} does not match agent_context.discovery");
                }
            }
            if original["onboarding_id"]
                .as_str()
                .filter(|s| !s.trim().is_empty())
                .is_none()
                || original["onboarding_id"] != context["subject"]["id"]
            {
                anyhow::bail!("onboarding.onboarding_id does not match agent_context.subject.id");
            }
            if id.trim().is_empty()
                || !hash.strip_prefix("sha256:").is_some_and(|value| {
                    value.len() == 64
                        && value
                            .bytes()
                            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
                })
            {
                anyhow::bail!(
                    "onboarding discovery identity requires a nonempty id and SHA-256 hash"
                );
            }
        }
        Ok(Some(Self {
            controller_bound,
            discovery_id: id.into(),
            discovery_hash: hash.into(),
        }))
    }

    pub(super) fn bind(&self, document: &Value) -> Result<Value, ToolError> {
        let mut bound = document.clone();
        if self.controller_bound {
            let object = bound
                .as_object_mut()
                .ok_or_else(|| invalid("proposal must be an object"))?;
            for field in object.keys() {
                if IDENTITY_FIELDS.contains(&field.as_str()) {
                    return Err(invalid(format!(
                        "proposal.{field} is controller-owned; omit it from this submission"
                    )));
                }
                if !PROPOSAL_FIELDS.contains(&field.as_str()) {
                    return Err(invalid(format!(
                        "proposal.{field} is not supported by the saved submission contract"
                    )));
                }
            }
            for field in PROPOSAL_FIELDS {
                if !object.contains_key(field) {
                    return Err(invalid(format!("proposal.{field} is required")));
                }
            }
            object.insert(
                "schema_version".into(),
                json!(pharness_core::ONBOARDING_PROPOSAL_SCHEMA),
            );
            object.insert("discovery_id".into(), json!(self.discovery_id));
            object.insert("discovery_hash".into(), json!(self.discovery_hash));
        }
        let proposal: RepositoryOnboardingProposal = serde_json::from_value(bound.clone())
            .map_err(|error| {
                invalid(format!(
                    "repository onboarding proposal is invalid: {error}"
                ))
            })?;
        for (field, supplied, expected) in [
            ("discovery_id", &proposal.discovery_id, &self.discovery_id),
            (
                "discovery_hash",
                &proposal.discovery_hash,
                &self.discovery_hash,
            ),
        ] {
            if supplied != expected {
                return Err(invalid(format!(
                    "proposal.{field} does not match the original Run discovery"
                )));
            }
        }
        proposal.validate_submission().map_err(invalid)?;
        Ok(bound)
    }
}

fn invalid(message: impl Into<String>) -> ToolError {
    ToolError::InvalidArguments {
        message: message.into(),
    }
}

#[cfg(test)]
mod tests;
