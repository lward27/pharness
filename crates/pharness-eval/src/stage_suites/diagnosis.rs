//! Observed Test receipts and immutable source snapshots. Failure type is not
//! the same as prior history, causal confidence, or the recommended repair size.
use super::{measurement, StageFixture};
use anyhow::{bail, Result};
use serde_json::json;
use std::path::Path;

pub(super) fn populate(fixture: &mut StageFixture) {
    let category = fixture.expected["classification"].as_str().unwrap();
    let mut source = "VALUE = 1\n".to_string();
    let mut tests = "import unittest\nfrom src.app import VALUE\nclass AppTest(unittest.TestCase):\n    def test_value(self): self.assertEqual(VALUE, 1)\n".to_string();
    let unit = json!([
        "python3",
        "-m",
        "unittest",
        "discover",
        "-s",
        "tests",
        "-p",
        "test_app.py",
        "-v"
    ]);
    let (kind, name, args, deadline) = match category {
        "assertion_failure" | "preexisting_failure" | "localized_repair" | "coherent_repair" => {
            tests = tests.replace("assertEqual(VALUE, 1)", "assertEqual(VALUE, 2)");
            if category == "coherent_repair" {
                tests.push_str(
                    "    def test_second_consumer(self): self.assertEqual(VALUE * 2, 4)\n",
                );
            }
            ("assertion", "unit", unit, 5_000)
        }
        "compile_failure" => {
            source = "def current_value(:\n    return 1\n".into();
            (
                "compilation",
                "compile",
                json!(["python3", "-m", "py_compile", "src/app.py"]),
                5_000,
            )
        }
        "lint_failure" => {
            source = "import os\nVALUE = 1\n".into();
            (
                "lint",
                "lint",
                json!(["python3", "tests/check_imports.py"]),
                5_000,
            )
        }
        "semantic_test_failure" => {
            source = "def accepts(value):\n    return bool(value)\n".into();
            tests = "import unittest\nfrom src.app import accepts\nclass AppTest(unittest.TestCase):\n    def test_nonempty(self): self.assertTrue(accepts('valid'))\n".into();
            ("semantic_test", "unit", unit, 5_000)
        }
        "acceptance_evidence_mismatch" => (
            "structural_environment",
            "compile",
            json!(["python3", "-m", "py_compile", "src/app.py"]),
            5_000,
        ),
        "tool_timeout" => {
            tests = "import time\nimport unittest\nclass AppTest(unittest.TestCase):\n    def test_wait(self): time.sleep(5); self.assertTrue(True)\n".into();
            ("structural_environment", "unit", unit, 100)
        }
        "environment_failure" => (
            "structural_environment",
            "unit",
            json!(["./.missing-test-executable"]),
            5_000,
        ),
        "contract_failure" => ("structural_environment", "unit", unit, 5_000),
        "no_failure" => ("unknown", "unit", unit, 5_000),
        _ => unreachable!("compiled diagnosis category"),
    };
    fixture.task = "Diagnose the observed deterministic Test result using its source-bound controller evidence. Distinguish failure type from history and repair scope. Preserve preexisting failures, identify missing or incompatible evidence, and do not propose repair when no failure is evidenced. Do not modify source; submit one typed diagnosis.".into();
    fixture.context = json!({"selected_acceptance_names":[if category == "acceptance_evidence_mismatch" {"unit"} else {name}],"requirements":if category == "semantic_test_failure" {"accepts must reject empty and whitespace-only input. Commands must match the selected acceptance names and pinned lock."} else if matches!(category,"assertion_failure"|"preexisting_failure"|"localized_repair"|"coherent_repair") {"VALUE must equal 2 for both consumers. Commands must match selected acceptance names and the pinned lock. Preserve observed baseline failures."} else {"Preserve VALUE = 1, valid Python syntax and the declared unused-import check. Commands must match selected acceptance names and the pinned lock. Only evidenced failures justify a repair."}});
    fixture.evidence = json!({});
    fixture.expected["failure_kind"] = json!(kind);
    fixture.expected["command"] = json!({"name":name,"argv":args,"timeout_ms":deadline});
    fixture.expected["workspace_files"] = json!({
        "README.md":"# Test contract\nUse the recorded test receipts and immutable source.\n",
        "requirements.lock":"# Declared typing support; behavioral tests use the standard library only.\ntyping-extensions==4.15.0 --hash=sha256:f0fa19c6845758ab08074a0cfa8b7aecb71c999ca73d62883bc25cc018c4e548\n",
        "src/app.py":source,"tests/test_app.py":tests,
        "tests/check_imports.py":"import ast\nfrom pathlib import Path\ntree=ast.parse(Path('src/app.py').read_text())\nused={node.id for node in ast.walk(tree) if isinstance(node,ast.Name)}\nunused=[alias.asname or alias.name for node in ast.walk(tree) if isinstance(node,ast.Import) for alias in node.names if (alias.asname or alias.name) not in used]\nfor name in unused: print('src/app.py: unused import '+name)\nraise SystemExit(1 if unused else 0)\n"
    });
}

pub(super) fn prepare(root: &Path, fixture: &StageFixture) -> Result<StageFixture> {
    let mut prepared = fixture.clone();
    let category = fixture.expected["classification"].as_str().unwrap();
    let command = &fixture.expected["command"];
    let baseline = if category == "preexisting_failure" {
        let baseline_hash = measurement::source_hash(root)?;
        let mut baseline = measurement::run_receipt(root, &command["argv"], 5_000)?;
        baseline["source_content_hash"] = json!(baseline_hash);
        std::fs::write(root.join("README.md"), "# Test contract\nDocument the existing source behavior; this change only adds this explanation.\n")?;
        Some(baseline)
    } else {
        None
    };
    let contract_value = measurement::contract(
        &json!({"candidate_files":fixture.expected["workspace_files"],"environment_profile":"python-3.11","commands":[command]}),
        &json!(["src/**", "tests/**", "README.md"]),
    )?;
    let mut native_refusal = None;
    if category == "contract_failure" {
        let native_contract: pharness_core::RepositoryContract =
            serde_json::from_value(contract_value.clone())?;
        native_contract.validate(root)?;
        std::fs::write(
            root.join("requirements.lock"),
            "# Unrecorded dependency change\n",
        )?;
        let error = native_contract
            .validate(root)
            .expect_err("fixture mutates the sealed lock")
            .to_string();
        if !error.contains("dependency lock SHA-256") {
            bail!("unexpected native validation failure: {error}");
        }
        native_refusal = Some(
            json!({"executed":false,"status":"refused","check":"RepositoryContract::validate","pinned_lock_hash":native_contract.dependency_lock.sha256,"error":error,"exit_code":null}),
        );
    }
    let hash = measurement::source_hash(root)?;
    let mut receipt = match native_refusal {
        Some(refusal) => refusal,
        None => measurement::run_receipt(
            root,
            &command["argv"],
            command["timeout_ms"].as_u64().unwrap(),
        )?,
    };
    let expected_status = match category {
        "tool_timeout" => "timed_out",
        "environment_failure" => "spawn_failed",
        "contract_failure" => "refused",
        _ => "completed",
    };
    if receipt["status"] != expected_status {
        bail!("diagnosis measurement prerequisite failed: {receipt}");
    }
    if receipt["status"] == "completed" {
        let expected_exit = if matches!(
            category,
            "semantic_test_failure" | "acceptance_evidence_mismatch" | "no_failure"
        ) {
            0
        } else {
            1
        };
        if receipt["exit_code"] != expected_exit {
            bail!("diagnosis source did not reproduce its recorded scenario: {receipt}");
        }
    }
    receipt["name"] = command["name"].clone();
    receipt["source_content_hash"] = json!(hash);
    let semantic = if category == "semantic_test_failure" {
        let spec = json!({"environment_profile":"python-3.11","oracle":"from src.app import accepts; assert accepts(' ') is False"});
        let observed = measurement::oracle_receipt(root, &spec, &hash)?;
        if observed["status"] != "completed" || observed["exit_code"] != 1 {
            bail!("semantic diagnosis fixture did not reproduce its defect");
        }
        Some(observed)
    } else {
        None
    };
    prepared.evidence = json!({"schema_version":"pharness.dev/stage-measurement/v1","requirements":fixture.context["requirements"],"selected_acceptance_names":fixture.context["selected_acceptance_names"],"test_receipts":[receipt],"baseline_receipt":baseline,"semantic_receipt":semantic,"meaning":"Observed source and command facts. Failure classification and repair recommendations are not supplied."});
    prepared.context["source_content_hash"] = json!(hash);
    prepared.context["repository_contract"] = contract_value;
    prepared.context["effective_upstream_outcomes"] =
        json!([{"stage":"test","status":"recorded","evidence_ref":"fixture_evidence"}]);
    prepared.expected["prepared_source_hash"] = json!(hash);
    prepared.expected["prepared_base_sha"] =
        json!(super::git_lines(root, &["rev-parse", "HEAD"])?.remove(0));
    Ok(prepared)
}

#[cfg(test)]
mod tests {
    use super::super::{fixtures, prepare_case, SuiteKind};

    #[test]
    fn every_diagnosis_snapshot_reproduces_its_actual_receipt_without_answer_categories() {
        let cases = fixtures(SuiteKind::TestDiagnosisV2).unwrap();
        assert_eq!(cases.len(), 12);
        for fixture in cases {
            let (prepared, root) = prepare_case(SuiteKind::TestDiagnosisV2, &fixture, 91).unwrap();
            assert!(prepared.evidence.get("classification").is_none());
            assert!(prepared
                .evidence
                .get("synthetic_qualification_receipt")
                .is_none());
            assert_ne!(
                prepared.evidence["test_receipts"][0]["status"],
                serde_json::Value::Null
            );
            assert_eq!(
                prepared.evidence["test_receipts"][0]["source_content_hash"],
                prepared.context["source_content_hash"]
            );
            if fixture.expected["classification"] == "preexisting_failure" {
                assert_eq!(prepared.evidence["baseline_receipt"]["exit_code"], 1);
                assert_ne!(
                    prepared.evidence["baseline_receipt"]["source_content_hash"],
                    prepared.context["source_content_hash"]
                );
            }
            std::fs::remove_dir_all(root).unwrap();
        }
    }
}
