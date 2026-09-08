use super::*;
use crate::stage_suites::{fixtures, replay_actions, validate_submission};
use pharness_core::AgentAction;

#[test]
fn onboarding_configuration_permissions_cannot_replace_future_development_scope() {
    let fixture = fixtures(SuiteKind::OnboardingV2)
        .unwrap()
        .into_iter()
        .find(|fixture| fixture.id == "python-contract")
        .unwrap();
    let mut proposal = replay_actions(SuiteKind::OnboardingV2, &fixture)
        .unwrap()
        .into_iter()
        .find_map(|action| match action {
            AgentAction::SubmitOnboardingProposal { proposal, .. } => Some(proposal),
            _ => None,
        })
        .unwrap();
    // The grader receives the controller-bound document, not the agent's
    // identity-free native tool arguments.
    proposal["schema_version"] = json!(pharness_core::ONBOARDING_PROPOSAL_SCHEMA);
    proposal["discovery_id"] = fixture.context["discovery"]["id"].clone();
    proposal["discovery_hash"] = fixture.context["discovery"]["hash"].clone();
    assert!(validate_submission(
        SuiteKind::OnboardingV2,
        &fixture,
        Some(&proposal),
        &[],
        &mut Vec::new()
    ));
    // This is the scope submitted by source-92's failed live canary. Preserve
    // its rejection; clarify the shared prompt instead of relaxing the grader.
    proposal["candidate_contract"]["writable_paths"] = json!([
        ".pharness/repository.yaml",
        ".pharness/instructions.md",
        ".pharness/project.yaml"
    ]);
    let mut violations = Vec::new();
    assert!(!validate_submission(
        SuiteKind::OnboardingV2,
        &fixture,
        Some(&proposal),
        &[],
        &mut violations
    ));
    assert!(violations.contains(&"ungrounded_development_write_scope".into()));
}

fn document(suite: SuiteKind, fixture: &StageFixture) -> Value {
    replay_actions(suite, fixture)
        .unwrap()
        .into_iter()
        .find_map(|action| match action {
            AgentAction::SubmitWorkPlan { work_plan, .. } => Some(work_plan),
            AgentAction::SubmitTestDiagnosis { diagnosis, .. } => Some(diagnosis),
            _ => None,
        })
        .unwrap()
}

#[test]
fn planner_warnings_do_not_grant_execution_and_cannot_hide_unbounded_steps() {
    let fixture = fixtures(SuiteKind::PlannerV2).unwrap().remove(1);
    let mut plan = document(SuiteKind::PlannerV2, &fixture);
    plan["risks"] = json!(["Do not run curl or npm install, and do not modify deploy/."]);
    let mut old = Vec::new();
    assert!(!validate_submission(
        SuiteKind::PlannerV1,
        &crate::stage_suites::planner_fixtures().unwrap().remove(1),
        Some(&plan),
        &[],
        &mut old
    ));
    assert!(old.contains(&"undeclared_command_or_path".into()));
    assert!(validate_planner(&fixture, &plan, &mut Vec::new()));
    for text in [
        "Do not use the forbidden deploy/** path, and do not add new paths.",
        "Do not run curl unless an operator independently authorizes a separate request.",
        "Retain the warning about npm install in the documentation.",
    ] {
        let mut warning = plan.clone();
        warning["steps"][0]["description"] = json!(text);
        assert!(validate_planner(&fixture, &warning, &mut Vec::new()));
    }
    for (field, value) in [
        ("title", Value::Null),
        ("risk_level", json!("unbounded")),
        ("risks", json!([123])),
        ("command", json!("curl")),
    ] {
        let mut changed = plan.clone();
        changed[field] = value;
        assert!(
            !validate_planner(&fixture, &changed, &mut Vec::new()),
            "{field}"
        );
    }
    for (pointer, value) in [
        ("/steps/0/paths", json!(["deploy/production.yaml"])),
        ("/steps/0/paths", json!(["src/../../private/file"])),
        ("/steps/0/paths", json!(["src-other/file"])),
        (
            "/steps/0/acceptance_names",
            json!(["unit", "compile", "npm install"]),
        ),
        ("/steps/0/acceptance_names", json!(["unit"])),
        ("/steps/0/acceptance_names", json!([])),
    ] {
        let mut changed = plan.clone();
        *changed.pointer_mut(pointer).unwrap() = value;
        assert!(
            !validate_planner(&fixture, &changed, &mut Vec::new()),
            "{pointer}"
        );
    }
    let mut changed = plan.clone();
    changed["steps"][0]["command"] = json!("curl");
    assert!(!validate_planner(&fixture, &changed, &mut Vec::new()));
    let mut changed = document(SuiteKind::PlannerV2, &fixture);
    changed["steps"][0]["acceptance_names"] = json!([]);
    changed["summary"] = json!(format!("{} unit compile", "intent"));
    assert!(
        !validate_planner(&fixture, &changed, &mut Vec::new()),
        "prose is not declared acceptance coverage"
    );
}

#[test]
fn diagnosis_replay_uses_the_actual_tool_contract_and_rejects_wrong_evidence_and_repairs() {
    let schema = pharness_runhost::worker_tool_specs()
        .into_iter()
        .find(|v| v.name == "submit_test_diagnosis")
        .unwrap()
        .parameters_schema;
    let diagnosis_schema = &schema["properties"]["diagnosis"];
    assert!(diagnosis_schema["properties"]
        .get("classification")
        .is_none());
    for fixture in fixtures(SuiteKind::TestDiagnosisV2).unwrap() {
        let good = document(SuiteKind::TestDiagnosisV2, &fixture);
        let props = diagnosis_schema["properties"].as_object().unwrap();
        assert!(good
            .as_object()
            .unwrap()
            .keys()
            .all(|key| props.contains_key(key)));
        for key in diagnosis_schema["required"].as_array().unwrap() {
            assert!(good.get(key.as_str().unwrap()).is_some());
        }
        assert!(props["failure_kind"]["enum"]
            .as_array()
            .unwrap()
            .contains(&good["failure_kind"]));
        assert!(validate_diagnosis(&fixture, &good, &mut Vec::new()));
        let mut plain = good.clone();
        plain["summary"] =
            json!("The observed result and selected command determine the submitted failure type.");
        assert!(validate_diagnosis(&fixture, &plain, &mut Vec::new()));
        // The previous scorer always rejected a schema-compliant submission.
        assert_ne!(good["classification"], fixture.expected["classification"]);
        for (key, value) in [
            ("failure_kind", json!("invented_kind")),
            ("summary", json!("")),
            ("evidence_refs", json!([])),
            (
                "evidence_refs",
                json!(["fixture_evidence", "invented_evidence"]),
            ),
            ("repair_recommendations", json!("untyped recommendation")),
            ("classification", fixture.expected["classification"].clone()),
        ] {
            let mut changed = good.clone();
            changed[key] = value;
            assert!(
                !validate_diagnosis(&fixture, &changed, &mut Vec::new()),
                "{} {key}",
                fixture.id
            );
        }
        if fixture.expected["classification"] == "no_failure" {
            let mut changed = good.clone();
            changed["repair_recommendations"] = json!(["Rewrite passing code"]);
            assert!(!validate_diagnosis(&fixture, &changed, &mut Vec::new()));
        }
    }
}

#[test]
fn corrected_suite_revisions_are_distinct_and_do_not_change_coding_or_repair_gates() {
    for (id, revision) in [
        ("onboarding-v2", "stage-qualification-v2.3"),
        ("planner-v2", "stage-qualification-v2.3"),
        ("test-diagnosis-v2", "stage-qualification-v2.3"),
        ("verifier-v2", "stage-qualification-v2.3"),
    ] {
        assert_eq!(
            pharness_core::inference_qualification_fixture_revision(id).unwrap(),
            revision
        );
        for old_revision in [
            "stage-qualification-v2.0",
            "stage-qualification-v2.1",
            "stage-qualification-v2.2",
        ] {
            if old_revision == revision {
                continue;
            }
            let old = pharness_core::canonical_json_sha256(&json!({
                "schema_version":pharness_core::INFERENCE_QUALIFICATION_SUITE_SCHEMA,
                "suite_id":id,"fixture_revision":old_revision
            }))
            .unwrap();
            assert_ne!(
                old,
                pharness_core::inference_qualification_suite_hash(id).unwrap()
            );
        }
    }
    for (id, revision) in [
        ("coding-v2", "coding-reliability-v2.1"),
        ("repair-v2", "repair-reliability-v2.1"),
    ] {
        assert_eq!(
            pharness_core::inference_qualification_fixture_revision(id).unwrap(),
            revision
        );
    }
}

#[test]
fn failed_stage_diagnostics_preserve_contract_fields_without_inventing_missing_results() {
    let suite = SuiteKind::TestDiagnosisV2;
    let fixture = fixtures(suite).unwrap().remove(0);
    let mut wrong = document(suite, &fixture);
    wrong["failure_kind"] = json!("unknown");
    let detail = submission_diagnostic(
        suite,
        &fixture,
        Some(&wrong),
        &["test_failure_misclassified".into()],
    );
    assert!(detail.contains("expected_failure_kind"));
    assert!(detail.contains("assertion"));
    assert!(detail.contains("unknown"));
    assert!(!submission_diagnostic(suite, &fixture, None, &[]).contains("assertion"));
    wrong["evidence_refs"] = json!(["authorization: Bearer diagnostic-canary"]);
    assert!(
        !submission_diagnostic(suite, &fixture, Some(&wrong), &[]).contains("diagnostic-canary")
    );
}
