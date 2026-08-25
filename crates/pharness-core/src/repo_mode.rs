use crate::RunBudget;
use serde::{Deserialize, Serialize};

pub const STAGE_OUTCOME_SCHEMA: &str = "pharness.dev/stage-outcome/v1alpha1";
pub const EVIDENCE_VALIDATION_SCHEMA: &str = "pharness.dev/evidence-validation/v1alpha1";
pub const AGENT_CONTEXT_SCHEMA: &str = "pharness.dev/agent-context/v1alpha1";
pub const ONBOARDING_PROPOSAL_SCHEMA: &str = "pharness.dev/repository-onboarding-proposal/v1alpha1";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RepoStageKey {
    Discover,
    Plan,
    Implement,
    Test,
    Verify,
    SourceDelivery,
    Release,
    Observe,
}

impl RepoStageKey {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Discover => "discover",
            Self::Plan => "plan",
            Self::Implement => "implement",
            Self::Test => "test",
            Self::Verify => "verify",
            Self::SourceDelivery => "source_delivery",
            Self::Release => "release",
            Self::Observe => "observe",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StageTerminalStatus {
    Succeeded,
    Failed,
    Blocked,
    Cancelled,
    Inapplicable,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RepositoryOnboardingProposal {
    pub schema_version: String,
    pub discovery_id: String,
    pub discovery_hash: String,
    pub candidate_contract: serde_json::Value,
    pub instructions: String,
    #[serde(default)]
    pub service_proposals: Vec<serde_json::Value>,
    #[serde(default)]
    pub binding_proposals: Vec<serde_json::Value>,
    #[serde(default)]
    pub assumptions: Vec<String>,
    #[serde(default)]
    pub conflicts: Vec<String>,
    #[serde(default)]
    pub blockers: Vec<String>,
    pub readiness_forecast: serde_json::Value,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StageOutcomeDocument {
    pub schema_version: String,
    pub work_item_id: String,
    pub stage_execution_id: String,
    pub stage: RepoStageKey,
    pub status: StageTerminalStatus,
    pub objective: serde_json::Value,
    pub pinned_inputs: serde_json::Value,
    #[serde(default)]
    pub verified_facts: Vec<serde_json::Value>,
    #[serde(default)]
    pub agent_claims: Vec<serde_json::Value>,
    #[serde(default)]
    pub outputs: Vec<serde_json::Value>,
    #[serde(default)]
    pub acceptance: Vec<serde_json::Value>,
    #[serde(default)]
    pub decisions: Vec<serde_json::Value>,
    #[serde(default)]
    pub authorizations: Vec<serde_json::Value>,
    #[serde(default)]
    pub contradictions: Vec<serde_json::Value>,
    #[serde(default)]
    pub risks: Vec<serde_json::Value>,
    #[serde(default)]
    pub unavailable_capabilities: Vec<serde_json::Value>,
    #[serde(default)]
    pub recommendations: Vec<serde_json::Value>,
    pub stop_reason: String,
    pub sealed_state_version: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentProfile {
    pub id: String,
    pub version: String,
    pub profile_hash: String,
    pub prompt_version: String,
    pub model: String,
    pub tools: Vec<String>,
    pub budget: RunBudget,
}

pub fn compiled_agent_profiles(model: &str, prompt_version: &str) -> Vec<AgentProfile> {
    let specs = [
        (
            "repository-onboarding-proposer",
            16,
            24,
            100_000,
            200_000,
            600,
            vec![
                "read_file",
                "list_dir",
                "search_files",
                "submit_onboarding_proposal",
                "finish",
            ],
        ),
        (
            "repo-planner",
            16,
            24,
            100_000,
            200_000,
            600,
            vec!["get_evidence", "submit_work_plan", "finish"],
        ),
        (
            "repo-builder",
            48,
            100,
            400_000,
            1_000_000,
            3_600,
            vec![
                "environment_info",
                "get_evidence",
                "list_dir",
                "read_file",
                "search_files",
                "create_directory",
                "write_file",
                "patch_file",
                "run_acceptance_command",
                "git_diff",
                "git_status",
                "finish",
            ],
        ),
        (
            "repo-tester",
            8,
            12,
            80_000,
            160_000,
            900,
            vec![
                "get_evidence",
                "run_acceptance_command",
                "submit_test_outcome",
                "finish",
            ],
        ),
        (
            "repo-verifier",
            12,
            20,
            120_000,
            240_000,
            900,
            vec![
                "get_evidence",
                "read_file",
                "search_files",
                "git_diff",
                "git_status",
                "submit_verification",
                "finish",
            ],
        ),
    ];
    specs
        .into_iter()
        .map(
            |(id, initial_turns, hard_turns, initial_tokens, hard_tokens, seconds, tools)| {
                let budget = RunBudget {
                    initial_turns,
                    hard_turns,
                    initial_tokens,
                    hard_tokens,
                    active_execution_seconds: seconds,
                    verification_reserve_turns: if id == "repo-builder" { 8 } else { 0 },
                    ..RunBudget::default()
                };
                let material = serde_json::json!({
                    "id": id,
                    "version": "v1",
                    "prompt_version": prompt_version,
                    "model": model,
                    "tools": tools,
                    "budget": budget,
                });
                use sha2::{Digest, Sha256};
                let encoded =
                    serde_json::to_vec(&material).expect("compiled AgentProfile serializes");
                AgentProfile {
                    id: id.into(),
                    version: "v1".into(),
                    profile_hash: format!("sha256:{:x}", Sha256::digest(encoded)),
                    prompt_version: prompt_version.into(),
                    model: model.into(),
                    tools: tools.into_iter().map(str::to_string).collect(),
                    budget,
                }
            },
        )
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compiled_profiles_are_stable_and_role_scoped() {
        let first = compiled_agent_profiles("provider/model", "prompt-v1");
        let second = compiled_agent_profiles("provider/model", "prompt-v1");
        assert_eq!(first, second);
        assert_eq!(first.len(), 5);
        let tester = first
            .iter()
            .find(|profile| profile.id == "repo-tester")
            .unwrap();
        assert!(!tester.tools.iter().any(|tool| tool == "write_file"));
        let builder = first
            .iter()
            .find(|profile| profile.id == "repo-builder")
            .unwrap();
        assert!(builder.tools.iter().any(|tool| tool == "write_file"));
        assert!(first
            .iter()
            .all(|profile| profile.profile_hash.starts_with("sha256:")));
    }
}
