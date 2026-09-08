use super::*;
use crate::{profile_tool_names, tool_specs_for_run, ProjectTools, RunSpec};
use pharness_core::{AgentAction, ToolExecutor, PLANNER_SUBMISSION_CONTRACT};

fn run(current: bool) -> RunSpec {
    let profile = pharness_core::compiled_reliability_v2_agent_profiles(
        "fixture/model",
        crate::RELIABILITY_V2_PROMPT_BUNDLE_VERSION,
    )
    .into_iter()
    .find(|p| p.id == "repo-planner")
    .unwrap();
    let mut run: RunSpec = serde_json::from_value(json!({
        "run_id":"run_planner", "session_id":"session_planner", "cwd":std::env::current_dir().unwrap(),
        "user_task":"Plan a bounded change", "max_turns":profile.budget.initial_turns,
        "execution_target_json":{"agent_profile":profile,"repo_mode":{"stage":"plan"},"agent_context":{"schema_version":pharness_core::AGENT_CONTEXT_SCHEMA}}
    })).unwrap();
    if current {
        run.execution_target_json["planner_submission_contract"] =
            json!(PLANNER_SUBMISSION_CONTRACT);
    }
    let mut registry: pharness_core::InferenceRegistry =
        serde_json::from_str(include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../deploy/helm/pharness/files/inference-registry.json"
        )))
        .unwrap();
    registry.finalize_hashes().unwrap();
    let policy = registry.policy("planner-kimi-k3-v2", "v1").unwrap().clone();
    let target = registry
        .target(&policy.target.target_id, &policy.target.revision)
        .unwrap()
        .clone();
    let profile: pharness_core::AgentProfile =
        serde_json::from_value(run.execution_target_json["agent_profile"].clone()).unwrap();
    let mut binding = pharness_core::ResolvedInferenceBinding {
        schema_version: pharness_core::RESOLVED_INFERENCE_BINDING_SCHEMA.into(),
        target,
        policy: policy.clone(),
        prompt_version: crate::RELIABILITY_V2_PROMPT_BUNDLE_VERSION.into(),
        stage_prompt: Some(
            if current {
                current_prompt()
            } else {
                legacy_prompt()
            }
            .revision_record(),
        ),
        tool_schema_hash: pharness_core::canonical_json_sha256(
            &serde_json::to_value(
                tool_specs_for_run(&run, profile_tool_names(&run).unwrap().as_ref()).unwrap(),
            )
            .unwrap(),
        )
        .unwrap(),
        context_policy_hash: crate::context_policy_hash(
            InferenceStage::Plan,
            policy.max_input_tokens,
            policy.max_output_tokens,
        )
        .unwrap(),
        protocol_calibration_hash: pharness_core::canonical_json_sha256(
            &json!({"fixture":"protocol"}),
        )
        .unwrap(),
        profile_budget_hash: pharness_core::canonical_json_sha256(
            &serde_json::to_value(&profile.budget).unwrap(),
        )
        .unwrap(),
        base_agent_profile_hash: profile.profile_hash.clone(),
        agent_profile_hash: String::new(),
        binding_hash: String::new(),
    };
    binding.agent_profile_hash = binding.computed_agent_profile_hash().unwrap();
    binding.binding_hash = binding.computed_hash().unwrap();
    binding.validate().unwrap();
    run.inference = Some(crate::RunInferenceSpec {
        selection_id: "planner_selection".into(),
        stage_execution_id: "planner_stage".into(),
        binding,
        next_request_sequence: 1,
    });
    run
}

#[test]
fn saved_schema_survives_serialization_without_upgrading_legacy_runs() {
    for current in [false, true] {
        let run = run(current);
        let saved: RunSpec = serde_json::from_value(serde_json::to_value(&run).unwrap()).unwrap();
        let specs =
            tool_specs_for_run(&saved, profile_tool_names(&saved).unwrap().as_ref()).unwrap();
        let (messages, _) =
            crate::context_envelope::assemble(std::path::Path::new(&saved.cwd), &saved).unwrap();
        let delivered = serde_json::to_string(&messages).unwrap();
        assert_eq!(delivered.contains("Explicitly classify readiness"), current);
        assert_eq!(
            delivered.contains("State assumptions and contradictions"),
            !current
        );
        let plan = &specs
            .iter()
            .find(|s| s.name == "submit_work_plan")
            .unwrap()
            .parameters_schema["properties"]["work_plan"];
        assert_eq!(
            plan["required"]
                .as_array()
                .unwrap()
                .contains(&json!("readiness")),
            current
        );
        assert_eq!(plan["properties"].get("readiness").is_some(), current);
        if current {
            let hash = pharness_core::canonical_json_sha256(&serde_json::to_value(&specs).unwrap())
                .unwrap();
            let profile: pharness_core::AgentProfile =
                serde_json::from_value(saved.execution_target_json["agent_profile"].clone())
                    .unwrap();
            assert_eq!(
                hash,
                crate::constrained_tool_schema_hash(&profile.tools, &[], &[]).unwrap()
            );
        }
    }
}

#[tokio::test]
async fn current_submission_retains_blockers_and_rejects_silent_or_inconsistent_readiness() {
    let run = run(true);
    let tools = ProjectTools::for_run(std::path::Path::new(&run.cwd), &run).unwrap();
    for readiness in [
        None,
        Some(json!({"status":"ready","blockers":["Unresolved contract"]})),
        Some(json!({"status":"needs_decision","blockers":[]})),
    ] {
        let mut document = json!({"title":"Retain regression","summary":"Do not choose its behavior","steps":[{"title":"Inspect","description":"Locate the conflict"}],"risk_level":"medium"});
        if let Some(value) = readiness {
            document["readiness"] = value;
        }
        assert!(tools
            .execute(&AgentAction::SubmitWorkPlan {
                id: "invalid".into(),
                reason: "fixture".into(),
                work_plan: document
            })
            .await
            .is_err());
    }
    let document = json!({"readiness":{"status":"needs_decision","blockers":["The existing regression conflicts with the implementation; resolve the intended contract."]}});
    let result = tools
        .execute(&AgentAction::SubmitWorkPlan {
            id: "blocked".into(),
            reason: "fixture".into(),
            work_plan: document.clone(),
        })
        .await
        .unwrap();
    assert_eq!(result.content["document"], document);
}

#[tokio::test]
async fn legacy_tool_submission_remains_readable_without_implying_readiness() {
    let run = run(false);
    let tools = ProjectTools::for_run(std::path::Path::new(&run.cwd), &run).unwrap();
    let document = json!({"summary":"Original historical contract"});
    assert!(tools
        .execute(&AgentAction::SubmitWorkPlan {
            id: "legacy".into(),
            reason: "fixture".into(),
            work_plan: document.clone()
        })
        .await
        .is_ok());
    assert!(pharness_core::PlannerReadiness::require_ready(&document).is_err());
}
