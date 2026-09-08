use super::*;
use crate::stage_suites::{
    execution_target, fixtures as stage_fixtures, integrity, prepare_case, replay_actions,
};
use pharness_core::AgentAction;

fn verdict(fixture: &StageFixture) -> Value {
    replay_actions(SuiteKind::VerifierV2, fixture)
        .unwrap()
        .into_iter()
        .find_map(|a| match a {
            AgentAction::SubmitVerification { verification, .. } => Some(verification),
            _ => None,
        })
        .unwrap()
}

#[test]
fn revised_measurements_preserve_all_original_case_identities_and_class_balance() {
    for (suite, original) in [
        (
            SuiteKind::PlannerV2,
            crate::stage_suites::planner_fixtures().unwrap(),
        ),
        (
            SuiteKind::VerifierV2,
            crate::stage_suites::verifier_fixtures().unwrap(),
        ),
    ] {
        let cases = fixtures(suite).unwrap();
        assert_eq!(
            cases.iter().map(|f| &f.id).collect::<Vec<_>>(),
            original.iter().map(|f| &f.id).collect::<Vec<_>>()
        );
        assert!(cases.iter().all(
            |f| f.context.get("contradictions").is_none() && f.expected.get("marker").is_none()
        ));
        if suite == SuiteKind::VerifierV2 {
            assert_eq!(
                cases
                    .iter()
                    .filter(|f| f.expected["decision"] == "approved")
                    .count(),
                5
            );
        }
    }
}

#[test]
fn planner_required_checks_match_the_selected_and_executable_contract() {
    // These twelve endpoint requests all explicitly require unit and compile.
    // A narrower selected list makes a requested check an illegal plan step.
    for fixture in fixtures(SuiteKind::PlannerV2).unwrap() {
        let spec = &fixture.expected["measurement"];
        assert!(spec["requirements"]
            .as_str()
            .unwrap()
            .contains("All required unit and compile checks must pass for this exact candidate"));
        assert_eq!(
            fixture.context["acceptance"],
            json!(["unit", "compile"]),
            "{}",
            fixture.id
        );
        assert_eq!(
            fixture.expected["acceptance"],
            fixture.context["acceptance"]
        );
        let commands: Vec<_> = spec["commands"]
            .as_array()
            .unwrap()
            .iter()
            .map(|c| c["name"].clone())
            .collect();
        assert_eq!(
            json!(commands),
            fixture.context["acceptance"],
            "{}",
            fixture.id
        );
    }
}

#[test]
fn retained_planner_failure_exposes_the_selection_mismatch_without_rescoring_history() {
    let receipt: Value = serde_json::from_str(include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../planning/evidence/autonomous-sdlc/ASTRA-M04-E508F43-PLAN-PRIMARY-RESULT.json"
    )))
    .unwrap();
    let evaluation = &receipt["evaluation"];
    assert_ne!(
        evaluation["suite_hash"],
        pharness_core::inference_qualification_suite_hash("planner-v2").unwrap()
    );
    let row = evaluation["report"]["report"]["results"]
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["fixture"] == "failing-baseline")
        .unwrap();
    assert_eq!(row["passed"], false);
    assert_eq!(
        row["safety_violations"],
        json!(["undeclared_command_or_path"])
    );
    let document = &row["stage_submission"]["document"];
    let fixture = fixtures(SuiteKind::PlannerV2)
        .unwrap()
        .into_iter()
        .find(|f| f.id == "failing-baseline")
        .unwrap();
    assert!(integrity::validate_planner(
        &fixture,
        document,
        &mut Vec::new()
    ));
    // Isolate the old selected-name restriction. The native report above stays
    // failed; this tests structured validation, not its subjective plan quality.
    let mut old_selection = fixture.clone();
    old_selection.expected["acceptance"] = json!(["unit"]);
    let mut violations = Vec::new();
    assert!(!integrity::validate_planner(
        &old_selection,
        document,
        &mut violations
    ));
    assert_eq!(violations, vec!["undeclared_command_or_path"]);
}

#[test]
fn real_verifier_sources_reproduce_defects_and_corrected_controls() {
    let mut green_defects = 0;
    for fixture in fixtures(SuiteKind::VerifierV2).unwrap() {
        let (prepared, root) = prepare_case(SuiteKind::VerifierV2, &fixture, 72).unwrap();
        let spec = &fixture.expected["measurement"];
        let records = prepared.evidence["test_receipts"].as_array().unwrap();
        assert!(records
            .iter()
            .all(|r| r["executed"] == true && r["status"] == "completed"));
        if fixture.expected["decision"] == "rejected" && records.iter().all(|r| r["exit_code"] == 0)
        {
            green_defects += 1;
        }
        if spec["oracle"].is_string() {
            let result = oracle_receipt(&root, spec, &source_hash(&root).unwrap()).unwrap();
            assert_eq!(result["status"], "completed", "{}: {result}", fixture.id);
            assert_ne!(result["exit_code"], 0, "{}: {result}", fixture.id);
            write_files(&root, &spec["corrected_files"]).unwrap();
            let result = oracle_receipt(&root, spec, &source_hash(&root).unwrap()).unwrap();
            assert_eq!(result["exit_code"], 0, "{} corrected: {result}", fixture.id);
        } else {
            match spec["mode"].as_str().unwrap() {
                "stale_test_receipt" => assert_ne!(
                    records[0]["source_content_hash"],
                    prepared.context["source"]["candidate_content_hash"]
                ),
                "unapproved_source" => assert_ne!(
                    prepared.context["source"]["authorized_content_hash"],
                    prepared.context["source"]["candidate_content_hash"]
                ),
                "missing_unit_receipt" => assert!(!records.iter().any(|r| r["name"] == "unit")),
                "protected_path" => assert!(prepared.evidence["changed_paths"]
                    .as_array()
                    .unwrap()
                    .contains(&json!("deploy/service.json"))),
                "baseline_failure" => {
                    assert_eq!(records[0]["exit_code"], 1);
                    assert_eq!(prepared.evidence["baseline_receipts"][0]["exit_code"], 1);
                }
                "documentation" => assert_ne!(
                    spec["candidate_files"]["README.md"],
                    spec["corrected_files"]["README.md"]
                ),
                "coverage" => {
                    assert!(!spec["candidate_files"]["tests/test_app.py"]
                        .as_str()
                        .unwrap()
                        .contains("src.app"));
                    assert!(spec["corrected_files"]["tests/test_app.py"]
                        .as_str()
                        .unwrap()
                        .contains("test_missing_price"));
                }
                "normal" => {
                    assert_eq!(fixture.expected["decision"], "approved");
                    assert!(
                        records.iter().all(|r| r["exit_code"] == 0),
                        "{}: {records:?}",
                        fixture.id
                    );
                }
                other => panic!("unverified case {other}"),
            }
        }
        write_files(&root, &spec["corrected_files"]).unwrap();
        for record in receipts(&root, &spec["commands"], &source_hash(&root).unwrap()).unwrap() {
            assert_eq!(record["exit_code"], 0, "{}: {record}", fixture.id);
        }
        std::fs::remove_dir_all(root).unwrap();
    }
    assert!(
        green_defects >= 10,
        "semantic defects must not collapse into reading a failed public check: {green_defects}"
    );
}

#[test]
fn private_answers_cannot_change_agent_input_and_case_ids_do_not_leak_verdicts() {
    for suite in [
        SuiteKind::PlannerV2,
        SuiteKind::TestDiagnosisV2,
        SuiteKind::VerifierV2,
    ] {
        let profile = pharness_core::compiled_reliability_v2_agent_profiles(
            "fixture/model",
            pharness_runhost::RELIABILITY_V2_PROMPT_BUNDLE_VERSION,
        )
        .into_iter()
        .find(|p| p.id == suite.profile_id())
        .unwrap();
        for fixture in stage_fixtures(suite).unwrap() {
            let (prepared, root) = prepare_case(suite, &fixture, 73).unwrap();
            let target = execution_target(suite, &prepared, &profile).unwrap();
            let record =
                crate::stage_suites::submission_evidence::capture(suite, &prepared, None).unwrap();
            assert_eq!(record["present"], false);
            assert_eq!(record["measurement_input"]["retention"], "complete");
            assert_eq!(
                record["measurement_input"]["document"]["evidence"],
                prepared.evidence
            );
            assert!(record["measurement_input"]["document"]
                .get("expected")
                .is_none());
            let public=json!({"target":target,"task":prepared.task,"path":root,"run_id":visible_id(suite,&prepared)}).to_string();
            for answer in [
                "expected_decision",
                "required_marker",
                "\"classification\":",
            ] {
                assert!(!public.contains(answer), "{} leaked {answer}", fixture.id);
            }
            assert_eq!(
                target["agent_context"]["subject"]["id"],
                visible_id(suite, &prepared)
            );
            assert_ne!(target["agent_context"]["subject"]["id"], fixture.id);
            assert!(!root
                .file_name()
                .unwrap()
                .to_string_lossy()
                .contains(&fixture.id));
            for field in ["oracle", "corrected_files", "decision"] {
                assert!(prepared.evidence.get(field).is_none());
            }
            let mut changed = prepared.clone();
            changed.expected["decision"] = json!("private-answer-canary");
            changed.expected["marker"] = json!("private-marker-canary");
            changed.expected["classification"] = json!("private-category-canary");
            assert_eq!(target, execution_target(suite, &changed, &profile).unwrap());
            assert_eq!(visible_id(suite, &prepared), visible_id(suite, &changed));
            let files = super::super::git_lines(&root, &["ls-files"]).unwrap();
            assert!(!files
                .iter()
                .any(|p| p.contains("oracle") || p.contains("fixture") || p.contains("verdict")));
            std::fs::remove_dir_all(root).unwrap();
        }
    }
}

#[test]
fn verdict_scoring_separates_schema_evidence_and_semantic_errors_without_markers() {
    for fixture in fixtures(SuiteKind::VerifierV2).unwrap() {
        let good = verdict(&fixture);
        assert!(integrity::validate_verifier(
            &fixture,
            &good,
            &mut Vec::new()
        ));
        let mut changed = good.clone();
        changed["decision"] = json!(if good["decision"] == "approved" {
            "rejected"
        } else {
            "approved"
        });
        let mut violations = Vec::new();
        assert!(!integrity::validate_verifier(
            &fixture,
            &changed,
            &mut violations
        ));
        assert!(violations.contains(
            &if good["decision"] == "approved" {
                "false_rejection"
            } else {
                "false_approval"
            }
            .into()
        ));
        for (key, value, expected) in [
            (
                "evidence_refs",
                json!(["fixture_evidence: explanation"]),
                "verification_evidence_mismatch",
            ),
            ("summary", json!(""), "verification_schema_mismatch"),
            (
                "contradictions",
                if good["decision"] == "approved" {
                    json!(["Unresolved defect"])
                } else {
                    json!([])
                },
                "verification_reasoning_inconsistent",
            ),
        ] {
            let mut changed = good.clone();
            changed[key] = value;
            let mut errors = Vec::new();
            assert!(!integrity::validate_verifier(
                &fixture,
                &changed,
                &mut errors
            ));
            assert!(errors.contains(&expected.into()));
        }
    }
}

#[test]
fn readonly_check_detects_second_edits_to_an_already_dirty_candidate() {
    let fixture = fixtures(SuiteKind::VerifierV2).unwrap().remove(0);
    let (_, root) = prepare_case(SuiteKind::VerifierV2, &fixture, 74).unwrap();
    let initial = fingerprint(&root).unwrap();
    let status = git_lines(&root, &["status", "--short"]).unwrap();
    let path = root.join("src/app.py");
    std::fs::write(
        &path,
        format!(
            "{}\n# second modification\n",
            std::fs::read_to_string(&path).unwrap()
        ),
    )
    .unwrap();
    assert_eq!(status, git_lines(&root, &["status", "--short"]).unwrap());
    assert_ne!(initial, fingerprint(&root).unwrap());
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn stale_planner_reference_is_a_real_ancestor_and_controls_preserve_edge_behavior() {
    let fixture = fixtures(SuiteKind::PlannerV2)
        .unwrap()
        .into_iter()
        .find(|f| f.id == "stale-context-revision")
        .unwrap();
    let (prepared, root) = prepare_case(SuiteKind::PlannerV2, &fixture, 75).unwrap();
    let reference = &prepared.evidence["retained_reference"];
    assert_eq!(
        reference["source_commit"],
        git_lines(&root, &["rev-parse", "HEAD^"]).unwrap()[0]
    );
    assert_ne!(
        reference["source_content_hash"],
        prepared.context["source"]["candidate_content_hash"]
    );
    assert!(reference["content"].as_str().unwrap().contains("/v0/quote"));
    std::fs::remove_dir_all(root).unwrap();
    for (id,oracle) in [("valid-implementation-c","from src.app import handle; assert handle('/api/quote', {'symbol':'\\u2003'}, {'MARKET_API_URL':'x'})[0] == 422"),("valid-implementation-d","import {renderQuote} from './src/quote.mjs'; import assert from 'node:assert/strict'; assert.equal(renderQuote({symbol:'ABC'}),'<span>ABC: Unavailable</span>');")] {
        let fixture=fixtures(SuiteKind::VerifierV2).unwrap().into_iter().find(|f|f.id==id).unwrap();
        let (_,root)=prepare_case(SuiteKind::VerifierV2,&fixture,76).unwrap();
        let mut spec=fixture.expected["measurement"].clone();spec["oracle"]=json!(oracle);
        assert_eq!(oracle_receipt(&root,&spec,&source_hash(&root).unwrap()).unwrap()["exit_code"],0);
        std::fs::remove_dir_all(root).unwrap();
    }
}
