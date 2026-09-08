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
        revision: "2026-09-08.1",
        stage: pharness_core::InferenceStage::Onboarding,
        content: r#"Use discovery evidence to propose repository configuration. Separate discovered facts, proposed configuration, assumptions, conflicts and blockers. Copy executable facts only from discovery and compatible profile descriptors; do not invent roots, commands, locks, Services or profile IDs. Distinguish this onboarding Run's allowed_source_changes from candidate_contract.writable_paths: the former bounds onboarding configuration changes; the latter proposes the repository scope for future development WorkItems. Derive that future scope from discovered source, test and documentation roots and any existing contract constraints. Writable paths match exactly unless they end in /**: lib/handler.py permits that file only; lib/** permits descendants recursively. Directory roots and writable expressions have different syntax: lib or lib/ does not grant descendant writes. Use canonical repository-relative expressions without trailing/repeated separators or . or .. components; never broaden an existing exact-file permission into a subtree. Do not copy the onboarding-only .pharness paths into the future development scope. Describe both scopes accurately in instructions; proposing future paths does not authorize this Run to edit them. service_proposals creates new Product Services only; do not list an existing Service there. Reuse existing keys in binding_proposals.service_keys with discovered repository scopes. Propose new Services only when supported by the request and discovery; use empty arrays when no creation or binding change is needed. Submit only the configuration and explanation fields exposed by submit_onboarding_proposal. The controller attaches schema_version, discovery_id and discovery_hash from this Run; omit those fields. If required facts are absent or contradictory, submit candidate_contract: null with explicit blockers or conflicts. A blocked proposal is retained evidence and cannot authorize source changes."#,
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
    properties["service_proposals"]["description"] = json!("Creates new Product Services only. Do not repeat an existing Product Service here. Reuse existing service_keys in binding_proposals instead; use [] when no new Service is proposed.");
    properties["service_proposals"]["maxItems"] = json!(32);
    properties["binding_proposals"]["description"] = json!("Binds this Repository to existing Product service_keys or separately proposed new Services. Referencing an existing Service here reuses it without creating it. Use [] when no binding change is proposed.");
    properties["binding_proposals"]["maxItems"] = json!(1);
    properties["binding_proposals"]["items"]["properties"]["scopes"]["minItems"] = json!(1);
    properties["binding_proposals"]["items"]["properties"]["scopes"]["maxItems"] = json!(64);
}

#[derive(Debug, Clone)]
pub(super) struct OnboardingBinding {
    controller_bound: bool,
    discovery_id: String,
    discovery_hash: String,
    product_service_keys: Option<Vec<String>>,
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
            product_service_keys: if controller_bound {
                product_service_keys(context)?
            } else {
                None
            },
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
        if self.controller_bound {
            if let Some(keys) = &self.product_service_keys {
                proposal
                    .validate_product_proposals(keys.iter().map(String::as_str))
                    .map_err(|error| invalid(error.to_string()))?;
            } else if !proposal.service_proposals.is_empty()
                || !proposal.binding_proposals.is_empty()
            {
                return Err(invalid("service_proposals and binding_proposals require the original Run's agent_context.product_model snapshot; do not invent Product state"));
            }
        }
        Ok(bound)
    }
}

fn product_service_keys(context: &Value) -> anyhow::Result<Option<Vec<String>>> {
    let Some(product) = context
        .get("product_model")
        .filter(|value| !value.is_null())
    else {
        return Ok(None);
    };
    // Production wraps the immutable model; retained diagnostic Runs use its
    // compact services view. An invalid wrapped model never falls back to that view.
    let model = product.get("model").unwrap_or(product);
    let services = model["services"].as_array().ok_or_else(|| {
        anyhow::anyhow!("agent_context.product_model requires an original services snapshot")
    })?;
    let mut keys = std::collections::BTreeSet::new();
    for service in services {
        let key = service["service_key"]
            .as_str()
            .filter(|key| !key.trim().is_empty())
            .ok_or_else(|| {
                anyhow::anyhow!("agent_context.product_model.services requires service_key")
            })?;
        if !keys.insert(key.to_string()) {
            anyhow::bail!("agent_context.product_model.services repeats service_key {key:?}");
        }
    }
    Ok(Some(keys.into_iter().collect()))
}

fn invalid(message: impl Into<String>) -> ToolError {
    ToolError::InvalidArguments {
        message: message.into(),
    }
}

#[cfg(test)]
mod tests;
