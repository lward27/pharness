use super::*;
use crate::{profile_tool_names, tool_specs_for_run, ProjectTools};
use pharness_core::{AgentAction, ToolExecutor};

fn run(controller_bound: bool) -> RunSpec {
    let profile = pharness_core::compiled_reliability_v2_agent_profiles(
        "fixture/model",
        crate::RELIABILITY_V2_PROMPT_BUNDLE_VERSION,
    )
    .into_iter()
    .find(|p| p.id == "repository-onboarding-proposer")
    .unwrap();
    let mut run: RunSpec = serde_json::from_value(json!({
        "run_id":"run_original", "session_id":"session_original",
        "cwd":std::env::current_dir().unwrap(), "user_task":"Propose bounded configuration.",
        "max_turns":profile.budget.initial_turns,
        "execution_target_json":{
            "agent_profile":profile,
            "onboarding":{"onboarding_id":"onboard_original", "discovery_id":"rdisc_original", "discovery_hash":format!("sha256:{}", "a".repeat(64))},
            "agent_context":{
                "schema_version":pharness_core::AGENT_CONTEXT_SCHEMA,
                "subject":{"kind":"repository_onboarding", "id":"onboard_original"},
                "discovery":{"id":"rdisc_original", "hash":format!("sha256:{}", "a".repeat(64))}
            }
        }
    })).unwrap();
    if controller_bound {
        run.execution_target_json["onboarding_submission_contract"] =
            json!(ONBOARDING_SUBMISSION_CONTRACT);
    }
    run
}

fn semantic_proposal(blocked: bool) -> Value {
    json!({
        "candidate_contract":if blocked {Value::Null} else {json!({
            "api_version":"pharness.dev/v1alpha1", "environment_profile":"python-3.11",
            "dependency_lock":{"kind":"pip_requirements", "path":"requirements.lock", "sha256":"b".repeat(64)},
            "writable_paths":["src/**", "tests/**"],
            "acceptance_commands":[{"name":"unit", "command":"python -m unittest discover -s tests -v"}],
            "roots":{"source":["src"], "tests":["tests"], "documentation":[]},
            "agent_network":"denied", "package_installation":"preparation_only"
        })},
        "instructions":"Use discovered facts; resolve missing prerequisites before execution.",
        "service_proposals":[], "binding_proposals":[], "assumptions":[], "conflicts":[],
        "blockers":if blocked {json!(["immutable_dependency_lock_missing"])} else {json!([])},
        "readiness_forecast":{"coding":"requires controller validation"}
    })
}

fn legacy_proposal(blocked: bool) -> Value {
    let mut proposal = semantic_proposal(blocked);
    proposal["schema_version"] = json!(pharness_core::ONBOARDING_PROPOSAL_SCHEMA);
    proposal["discovery_id"] = json!("rdisc_original");
    proposal["discovery_hash"] = json!(format!("sha256:{}", "a".repeat(64)));
    proposal
}

#[tokio::test]
async fn retained_control_rejects_existing_service_creation_at_submission() {
    let retained: Value = serde_json::from_str(include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../planning/evidence/autonomous-sdlc/ASTRA-M04-38A4282-ONBOARDING-CONTROL-RESULT.json"
    )))
    .unwrap();
    let row = retained["evaluation"]["report"]["report"]["results"]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["fixture"] == "python-contract")
        .unwrap();
    let mut proposal = row["stage_submission"]["document"].clone();
    let mut run = run(true);
    for field in ["discovery_id", "discovery_hash"] {
        run.execution_target_json["onboarding"][field] = proposal[field].clone();
    }
    run.execution_target_json["agent_context"]["discovery"] = json!({
        "id":proposal["discovery_id"], "hash":proposal["discovery_hash"]
    });
    run.execution_target_json["agent_context"]["product_model"] = row["stage_submission"]
        ["measurement_input"]["document"]["context"]["product_model"]
        .clone();
    for field in IDENTITY_FIELDS {
        proposal.as_object_mut().unwrap().remove(field);
    }
    let original = proposal.clone();
    let tools = ProjectTools::for_run(std::path::Path::new(&run.cwd), &run).unwrap();
    let error = tools
        .execute(&AgentAction::SubmitOnboardingProposal {
            id: "act_retained_duplicate".into(),
            reason: "reuse existing Service".into(),
            proposal: proposal.clone(),
        })
        .await
        .unwrap_err();
    assert!(matches!(error, ToolError::InvalidArguments { ref message }
        if message.contains("service_proposals[0].service_key")
        && message.contains("already exists") && message.contains("binding_proposals")));
    assert_eq!(
        proposal, original,
        "reject without rewriting the model proposal"
    );

    // The existing binding already expresses reuse. Do not create the Service again.
    proposal["service_proposals"] = json!([]);
    let accepted = tools
        .execute(&AgentAction::SubmitOnboardingProposal {
            id: "act_reuse_existing".into(),
            reason: "bind existing Service without creating it".into(),
            proposal: proposal.clone(),
        })
        .await
        .unwrap();
    assert_eq!(
        accepted.content["document"]["binding_proposals"],
        proposal["binding_proposals"]
    );
}

#[test]
fn product_checks_use_the_original_wrapped_snapshot_and_require_known_context() {
    let mut run = run(true);
    run.execution_target_json["agent_context"]["product_model"] = json!({
        "snapshot_id":"pmodel_original", "content_hash":"sha256:original",
        "model":{"services":[{"service_key":"finance-web"}]}
    });
    let binding = OnboardingBinding::for_run(&run).unwrap().unwrap();
    let mut proposal = semantic_proposal(false);
    proposal["binding_proposals"] = json!([{"service_keys":["finance-web"], "scopes":["src/**"]}]);
    assert!(binding.bind(&proposal).is_ok());
    run.execution_target_json["agent_context"]["product_model"]["model"]["services"] = json!([]);
    assert!(
        binding.bind(&proposal).is_ok(),
        "an existing Run retains its original snapshot"
    );
    assert!(OnboardingBinding::for_run(&run)
        .unwrap()
        .unwrap()
        .bind(&proposal)
        .is_err());

    run.execution_target_json["agent_context"]
        .as_object_mut()
        .unwrap()
        .remove("product_model");
    let missing = OnboardingBinding::for_run(&run).unwrap().unwrap();
    assert!(missing.bind(&semantic_proposal(true)).is_ok());
    assert!(
        matches!(missing.bind(&proposal), Err(ToolError::InvalidArguments { message })
        if message.contains("original Run") && message.contains("product_model"))
    );
    for bad_model in [
        json!({"model":null,"services":[{"service_key":"finance-web"}]}),
        json!({"model":{"services":[{}]}}),
        json!({"model":{"services":[{"service_key":"finance-web"},{"service_key":"finance-web"}]}}),
    ] {
        run.execution_target_json["agent_context"]["product_model"] = bad_model;
        assert!(OnboardingBinding::for_run(&run).is_err());
    }
}

#[tokio::test]
async fn native_submission_rejects_ambiguous_scope_and_accepts_explicit_revision() {
    let run = run(true);
    let tools = ProjectTools::for_run(std::path::Path::new(&run.cwd), &run).unwrap();
    let mut proposal = semantic_proposal(false);
    proposal["candidate_contract"]["writable_paths"] = json!(["src/", "tests/", "README.md"]);
    let before = proposal.clone();
    let error = tools
        .execute(&AgentAction::SubmitOnboardingProposal {
            id: "act_ambiguous_scope".into(),
            reason: "propose discovered scope".into(),
            proposal: proposal.clone(),
        })
        .await
        .unwrap_err();
    assert!(matches!(error, ToolError::InvalidArguments { ref message }
        if message.contains("writable_paths") && message.contains("noncanonical")
        && message.contains("/**")));
    assert_eq!(proposal, before);

    // Reading a historical document is still possible; it cannot authorize new work.
    let mut historical = legacy_proposal(false);
    historical["candidate_contract"] = proposal["candidate_contract"].clone();
    let retained: RepositoryOnboardingProposal = serde_json::from_value(historical).unwrap();
    assert!(retained.approvable_contract().is_err());
    assert_eq!(retained.candidate_contract, before["candidate_contract"]);

    proposal["candidate_contract"]["writable_paths"] = json!(["src/**", "tests/**", "README.md"]);
    let result = tools
        .execute(&AgentAction::SubmitOnboardingProposal {
            id: "act_explicit_scope".into(),
            reason: "explicitly propose recursive scope".into(),
            proposal: proposal.clone(),
        })
        .await
        .unwrap();
    assert_eq!(
        result.content["document"]["candidate_contract"],
        proposal["candidate_contract"]
    );
}

#[tokio::test]
async fn native_tool_binds_ready_and_blocked_proposals_without_changing_semantics() {
    let run = run(true);
    let tools = ProjectTools::for_run(std::path::Path::new(&run.cwd), &run).unwrap();
    for blocked in [false, true] {
        let submitted = semantic_proposal(blocked);
        let result = tools
            .execute(&AgentAction::SubmitOnboardingProposal {
                id: "act_submit".into(),
                reason: "propose from original discovery".into(),
                proposal: submitted.clone(),
            })
            .await
            .unwrap();
        let document = &result.content["document"];
        assert_eq!(document, &legacy_proposal(blocked));
        let durable: RepositoryOnboardingProposal =
            serde_json::from_value(document.clone()).unwrap();
        assert_eq!(durable.approvable_contract().is_err(), blocked);
        assert_eq!(
            submitted,
            semantic_proposal(blocked),
            "input is never mutated"
        );
    }
}

#[test]
fn saved_contract_selects_matching_tool_schema_and_profile_instruction() {
    let legacy = run(false);
    let current = run(true);
    let allowed = profile_tool_names(&current).unwrap();
    let legacy_specs = tool_specs_for_run(&legacy, allowed.as_ref()).unwrap();
    let current_specs = tool_specs_for_run(&current, allowed.as_ref()).unwrap();
    let current_proposal = &current_specs
        .iter()
        .find(|s| s.name == "submit_onboarding_proposal")
        .unwrap()
        .parameters_schema["properties"]["proposal"];
    assert_eq!(current_proposal["required"], json!(PROPOSAL_FIELDS));
    assert_eq!(
        current_proposal["properties"]["service_proposals"]["maxItems"],
        32
    );
    assert_eq!(
        current_proposal["properties"]["binding_proposals"]["maxItems"],
        1
    );
    assert!(
        current_proposal["properties"]["service_proposals"]["description"]
            .as_str()
            .unwrap()
            .contains("Creates new Product Services only")
    );
    for field in IDENTITY_FIELDS {
        assert!(current_proposal["properties"].get(field).is_none());
        assert!(!current_proposal["required"]
            .as_array()
            .unwrap()
            .contains(&json!(field)));
    }
    let legacy_proposal = &legacy_specs
        .iter()
        .find(|s| s.name == "submit_onboarding_proposal")
        .unwrap()
        .parameters_schema["properties"]["proposal"];
    for field in IDENTITY_FIELDS {
        assert!(legacy_proposal["properties"].get(field).is_some());
    }
    let current_hash =
        pharness_core::canonical_json_sha256(&serde_json::to_value(&current_specs).unwrap())
            .unwrap();
    let legacy_hash =
        pharness_core::canonical_json_sha256(&serde_json::to_value(&legacy_specs).unwrap())
            .unwrap();
    assert_ne!(current_hash, legacy_hash);
    assert_eq!(
        current_hash,
        crate::constrained_tool_schema_hash(
            &allowed.unwrap().into_iter().collect::<Vec<_>>(),
            &[],
            &[],
        )
        .unwrap()
    );
    assert!(crate::profile_instruction(&current)
        .unwrap()
        .unwrap()
        .contains("omit those fields"));
    assert!(!crate::profile_instruction(&legacy)
        .unwrap()
        .unwrap()
        .contains("omit those fields"));
}

#[test]
fn legacy_v1_and_v2_documents_keep_exact_identity_checks() {
    let binding = OnboardingBinding::for_run(&run(false)).unwrap().unwrap();
    assert!(binding.bind(&semantic_proposal(false)).is_err());
    for schema in [
        "pharness.dev/repository-onboarding-proposal/v1alpha1",
        pharness_core::ONBOARDING_PROPOSAL_SCHEMA,
    ] {
        let mut proposal = legacy_proposal(false);
        proposal["schema_version"] = json!(schema);
        assert_eq!(binding.bind(&proposal).unwrap(), proposal);
        for field in ["discovery_id", "discovery_hash"] {
            let mut stale = proposal.clone();
            stale[field] = json!("other");
            assert!(binding
                .bind(&stale)
                .unwrap_err()
                .to_string()
                .contains(&format!("proposal.{field}")));
        }
    }
}

#[test]
fn current_contract_rejects_supplied_identity_even_when_it_matches() {
    let binding = OnboardingBinding::for_run(&run(true)).unwrap().unwrap();
    for field in IDENTITY_FIELDS {
        for value in [legacy_proposal(false)[field].clone(), json!("conflicting")] {
            let mut proposal = semantic_proposal(false);
            proposal[field] = value;
            let error = binding.bind(&proposal).unwrap_err().to_string();
            assert!(
                error.contains(&format!("proposal.{field} is controller-owned")),
                "{error}"
            );
        }
    }
}

#[test]
fn missing_and_unknown_fields_have_precise_native_rejections() {
    let binding = OnboardingBinding::for_run(&run(true)).unwrap().unwrap();
    for field in PROPOSAL_FIELDS {
        let mut proposal = semantic_proposal(true);
        proposal.as_object_mut().unwrap().remove(field);
        assert!(binding
            .bind(&proposal)
            .unwrap_err()
            .to_string()
            .contains(&format!("proposal.{field} is required")));
    }
    let mut proposal = semantic_proposal(true);
    proposal["production_approved"] = json!(true);
    assert!(binding
        .bind(&proposal)
        .unwrap_err()
        .to_string()
        .contains("proposal.production_approved"));
    assert!(binding.bind(&Value::Null).is_err());
}

#[test]
fn native_binding_preserves_blockers_candidate_validation_and_limits() {
    let binding = OnboardingBinding::for_run(&run(true)).unwrap().unwrap();
    for (pointer, value) in [
        ("/blockers", json!([])),
        ("/blockers", json!([""])),
        ("/readiness_forecast", json!([])),
        ("/instructions", json!("x".repeat(32769))),
        (
            "/readiness_forecast",
            json!({"too_big":"x".repeat(128 * 1024)}),
        ),
    ] {
        let mut proposal = semantic_proposal(true);
        *proposal.pointer_mut(pointer).unwrap() = value;
        assert!(binding.bind(&proposal).is_err(), "{pointer}");
    }
    let mut invalid_candidate = semantic_proposal(false);
    invalid_candidate["candidate_contract"]["dependency_lock"]["sha256"] = json!("invented");
    assert!(binding.bind(&invalid_candidate).is_err());
}

#[test]
fn new_run_rejects_missing_conflicting_or_malformed_original_context_before_execution() {
    for pointer in [
        "/agent_context/discovery/id",
        "/agent_context/discovery/hash",
        "/agent_context/schema_version",
        "/agent_context/subject/id",
        "/onboarding/discovery_id",
        "/onboarding/discovery_hash",
        "/onboarding/onboarding_id",
        "/agent_profile/version",
        "/agent_profile/id",
        "/onboarding_submission_contract",
    ] {
        let mut run = run(true);
        *run.execution_target_json.pointer_mut(pointer).unwrap() = json!("wrong");
        assert!(OnboardingBinding::for_run(&run).is_err(), "{pointer}");
    }
    for field in ["id", "hash"] {
        let mut run = run(true);
        run.execution_target_json["agent_context"]["discovery"]
            .as_object_mut()
            .unwrap()
            .remove(field);
        assert!(OnboardingBinding::for_run(&run).is_err(), "{field}");
    }
    let mut malformed = run(true);
    malformed.execution_target_json["onboarding"]["discovery_hash"] = json!("sha256:bad");
    malformed.execution_target_json["agent_context"]["discovery"]["hash"] = json!("sha256:bad");
    assert!(OnboardingBinding::for_run(&malformed).is_err());
}

#[test]
fn reconstructed_run_retains_original_contract_and_discovery() {
    for current in [false, true] {
        let original = run(current);
        let saved = serde_json::to_value(&original).unwrap();
        let reconstructed: RunSpec = serde_json::from_value(saved).unwrap();
        let original_binding = OnboardingBinding::for_run(&original).unwrap().unwrap();
        let restored_binding = OnboardingBinding::for_run(&reconstructed).unwrap().unwrap();
        let proposal = if current {
            semantic_proposal(true)
        } else {
            legacy_proposal(true)
        };
        assert_eq!(
            original_binding.bind(&proposal).unwrap(),
            restored_binding.bind(&proposal).unwrap()
        );
        let mut newer_run = original.clone();
        newer_run.execution_target_json["agent_context"]["discovery"]["id"] = json!("rdisc_newer");
        newer_run.execution_target_json["onboarding"]["discovery_id"] = json!("rdisc_newer");
        assert_eq!(
            restored_binding.bind(&proposal).unwrap()["discovery_id"],
            "rdisc_original"
        );
        let refreshed = OnboardingBinding::for_run(&newer_run).unwrap().unwrap();
        if current {
            assert_eq!(
                refreshed.bind(&proposal).unwrap()["discovery_id"],
                "rdisc_newer"
            );
        } else {
            assert!(refreshed.bind(&proposal).is_err());
        }
    }
}
