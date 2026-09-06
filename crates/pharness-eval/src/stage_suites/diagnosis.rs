//! Internally consistent synthetic Test receipts and immutable source snapshots.
//! Failure type, causal confidence and repair scope are different concepts.
use super::StageFixture;
use serde_json::json;

pub(super) fn populate(fixture: &mut StageFixture) {
    let category = fixture.expected["classification"].as_str().unwrap();
    let mut source = "VALUE = 1\n".to_string();
    let mut tests = "import unittest\nfrom src.app import VALUE\nclass AppTest(unittest.TestCase):\n    def test_value(self): self.assertEqual(VALUE, 1)\n".to_string();
    let unit = "python3 -m unittest discover -s tests -p test_app.py -v";
    let (kind, command, exit_code, output) = match category {
        "assertion_failure" | "preexisting_failure" | "localized_repair" | "coherent_repair" => {
            tests = tests.replace("assertEqual(VALUE, 1)", "assertEqual(VALUE, 2)");
            if category == "coherent_repair" {
                tests.push_str("    def test_second_consumer(self): self.assertEqual(VALUE * 2, 4)\n");
            }
            ("assertion", unit, Some(1), if category == "coherent_repair" {
                "FAIL: test_value (AppTest): AssertionError: 1 != 2\nFAIL: test_second_consumer (AppTest): AssertionError: 2 != 4\nBoth failures consume src.app.VALUE."
            } else {
                "FAIL: test_value (AppTest): AssertionError: 1 != 2\nThe test reads VALUE from src/app.py."
            })
        }
        "compile_failure" => {
            source = "def current_value(:\n    return 1\n".into();
            ("compilation", "python3 -m py_compile src/app.py", Some(1), "File src/app.py, line 1\n    def current_value(:\nSyntaxError: invalid syntax")
        }
        "lint_failure" => {
            source = "import os\nVALUE = 1\n".into();
            ("lint", "ruff check src tests", Some(1), "src/app.py:1:8: F401 os imported but unused")
        }
        "semantic_test_failure" => {
            source = "def accepts(value):\n    return bool(value)\n".into();
            tests = "import unittest\nfrom src.app import accepts\nclass AppTest(unittest.TestCase):\n    def test_nonempty(self): self.assertTrue(accepts('valid'))\n".into();
            ("semantic_test", unit, Some(0), "Public acceptance: Ran 1 test, OK.\nController semantic check: accepts(' ') must be False; observed True. Whitespace-only input is invalid under the sealed acceptance criterion.")
        }
        "acceptance_evidence_mismatch" => ("structural_environment", "python3 -m py_compile src/app.py", Some(0), "Selected acceptance name: unit. Received successful command evidence for compile only. Required unit evidence is missing; do not infer a unit-test pass from compilation."),
        "tool_timeout" => {
            tests = "import time\nimport unittest\nclass AppTest(unittest.TestCase):\n    def test_wait(self): time.sleep(5); self.assertTrue(True)\n".into();
            ("structural_environment", unit, None, "Controller deadline of 100 ms elapsed while executing the declared unit command. The process was terminated; no completed test result is available.")
        }
        "environment_failure" => ("structural_environment", "pytest -q", None, "The declared executable pytest is unavailable in the pinned test environment. The command did not start; no assertion, compilation or semantic result was produced."),
        "contract_failure" => ("structural_environment", unit, None, "The controller refused to start acceptance: the observed dependency-lock hash differs from the sealed RepositoryContract. No test command ran. Preserve the mismatch; do not rewrite the lock or change the environment."),
        "no_failure" => ("unknown", unit, Some(0), "Ran 1 test, OK. All selected acceptance evidence is present and consistent. No failure or repair is indicated."),
        _ => unreachable!("compiled diagnosis fixture category"),
    };
    fixture.task = "Diagnose the recorded deterministic Test result using its exact controller evidence. Distinguish the observed failure type from whether a cause is known, preexisting, or local to one repair. Do not modify source; submit one typed diagnosis.".into();
    fixture.context["selected_acceptance_names"] = json!(["unit"]);
    if matches!(category, "compile_failure" | "lint_failure") {
        fixture.context["selected_acceptance_names"] = json!([if category == "compile_failure" {
            "compile"
        } else {
            "lint"
        }]);
    }
    fixture.evidence = json!({
        "synthetic_qualification_receipt":true,
        "classification":category,"command":command,"exit_code":exit_code,
        "bounded_output":output,
        "baseline":if category == "preexisting_failure" {json!({"exit_code":1,"bounded_output":output,"same_failure_before_current_change":true})} else {json!(null)},
        "repair_scope":match category {"localized_repair"=>"one failing assertion","coherent_repair"=>"two consumers of the same incorrect value",_=>"only the failure supported by this receipt"},
    });
    fixture.expected["failure_kind"] = json!(kind);
    fixture.expected["workspace_files"] = json!({
        "README.md":"# Deterministic Test diagnosis fixture\nUse the sealed Test receipt and immutable source. A path or class name is not evidence of a failure cause.\n",
        "requirements.lock":"# No third-party dependency is needed to inspect this snapshot.\n",
        "src/app.py":source,"tests/test_app.py":tests,
    });
}

#[cfg(test)]
mod tests {
    use super::super::{fixtures, prepare_workspace, SuiteKind};
    use std::process::Command;

    #[test]
    fn diagnosis_snapshots_reproduce_assertion_compile_and_passing_receipts() {
        let cases = fixtures(SuiteKind::TestDiagnosisV2).unwrap();
        assert_eq!(cases.len(), 12);
        for fixture in cases {
            assert_eq!(fixture.evidence["synthetic_qualification_receipt"], true);
            let category = fixture.expected["classification"].as_str().unwrap();
            if !matches!(
                category,
                "assertion_failure"
                    | "preexisting_failure"
                    | "localized_repair"
                    | "coherent_repair"
                    | "compile_failure"
                    | "no_failure"
            ) {
                continue;
            }
            let root = prepare_workspace(SuiteKind::TestDiagnosisV2, &fixture, 91).unwrap();
            let args = fixture.evidence["command"]
                .as_str()
                .unwrap()
                .split_whitespace()
                .collect::<Vec<_>>();
            let output = Command::new(args[0])
                .args(&args[1..])
                .current_dir(&root)
                .env("PYTHONDONTWRITEBYTECODE", "1")
                .output()
                .unwrap();
            assert_eq!(
                output.status.code().map(i64::from),
                fixture.evidence["exit_code"].as_i64(),
                "{}",
                fixture.id
            );
            let stderr = String::from_utf8_lossy(&output.stderr);
            assert!(
                stderr.contains(if category == "compile_failure" {
                    "SyntaxError"
                } else if category == "no_failure" {
                    "OK"
                } else {
                    "AssertionError"
                }),
                "{}: {stderr}",
                fixture.id
            );
            if category == "preexisting_failure" {
                assert_eq!(
                    fixture.evidence["baseline"]["bounded_output"],
                    fixture.evidence["bounded_output"]
                );
            }
            std::fs::remove_dir_all(root).unwrap();
        }
    }
}
