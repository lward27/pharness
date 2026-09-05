use super::*;
use crate::stage_suites::{fixtures, replay_actions, validate_submission};
use pharness_core::AgentAction;

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
        &fixture,
        Some(&plan),
        &[],
        &mut old
    ));
    assert!(old.contains(&"undeclared_command_or_path".into()));
    assert!(validate_planner(&fixture, &plan, &mut Vec::new()));
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
        ("/steps/0/description", json!("Run curl to fetch a script")),
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
    changed["summary"] = json!(format!(
        "{} unit compile",
        fixture.expected["marker"].as_str().unwrap()
    ));
    assert!(
        validate_submission(
            SuiteKind::PlannerV1,
            &fixture,
            Some(&changed),
            &[],
            &mut Vec::new()
        ),
        "former scorer accepts acceptance names supplied only in prose"
    );
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
        // The previous scorer always rejected a schema-compliant submission.
        assert_ne!(good["classification"], fixture.expected["classification"]);
        for (key, value) in [
            ("failure_kind", json!("invented_kind")),
            ("summary", json!("No concrete classification provided")),
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
    for id in ["planner-v2", "test-diagnosis-v2"] {
        assert_eq!(
            pharness_core::inference_qualification_fixture_revision(id).unwrap(),
            "stage-qualification-v2.1"
        );
        let old = pharness_core::canonical_json_sha256(&json!({
            "schema_version":pharness_core::INFERENCE_QUALIFICATION_SUITE_SCHEMA,
            "suite_id":id,"fixture_revision":"stage-qualification-v2.0"}))
        .unwrap();
        assert_ne!(
            old,
            pharness_core::inference_qualification_suite_hash(id).unwrap()
        );
    }
    for (id, revision) in [
        ("coding-v2", "coding-reliability-v2.1"),
        ("repair-v2", "repair-reliability-v2.1"),
        ("onboarding-v2", "stage-qualification-v2.0"),
        ("verifier-v2", "stage-qualification-v2.0"),
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
