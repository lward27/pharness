use super::{retained_inputs, validate_report};
use pharness_core::{
    canonical_json_sha256, inference_qualification_suite_hash, InferenceEvaluationScope,
};
use pharness_store::StoredInferenceEvaluation;
use serde_json::{json, Value};

fn fixture() -> (StoredInferenceEvaluation, Value) {
    let registry = pharness_config::InferenceGatewayConfig::legacy_default().registry;
    let scope = InferenceEvaluationScope::Diagnostic {
        case_ids: vec!["acceptance-boundary".into()],
        reference_evaluation_id: None,
    };
    let input = json!({"task":"A bounded change","context":{"source":"original"},"evidence":{"receipt":"observed"}});
    let report = json!({
        "scope":scope,"gate_passed":false,"candidate_safe":false,"provider":"gateway",
        "diagnostic":{"case_ids":scope.case_ids(),"passed":true,"reference_evaluation_id":null},
        "report":{"resolved_settings":{"evaluation_scope":scope},"results":[{
            "fixture":"acceptance-boundary","attempt":1,"passed":true,"protected_paths_ok":true,"safety_violations":[],
            "workspace_hash":format!("sha256:{}","a".repeat(64)),"source_sha":"a".repeat(40),
            "stage_submission":{"document":{"model_output":"must not be reused"},"measurement_input":{"retention":"complete","document":input,"raw_content_sha256":canonical_json_sha256(&input).unwrap()}}
        }]}
    });
    let evaluation = serde_json::from_value(json!({
        "scope":scope,"id":"infeval_original","status":"completed","suite_id":"planner-v2",
        "suite_hash":inference_qualification_suite_hash("planner-v2").unwrap(),"attempts":1,
        "agent_profile_id":"repo-planner","agent_profile_hash":"profile","target_id":"target","target_revision":"v1","target_hash":"target",
        "policy_id":"policy","policy_revision":"v1","policy_hash":"policy","binding_hash":"binding",
        "resolved_binding":{"schema_version":pharness_core::RESOLVED_INFERENCE_BINDING_SCHEMA,"target":registry.targets[0],"policy":registry.policies[0],"prompt_version":"test","base_agent_profile_hash":"base","agent_profile_hash":"profile","tool_schema_hash":"tools","context_policy_hash":"context","profile_budget_hash":"budget","binding_hash":"binding"},
        "runtime_revision":"runtime","actor":"operator","reason":"diagnostic","config_hash":"config","job_name":"job",
        "report":report,"report_hash":canonical_json_sha256(&report).unwrap(),"created_at":"then","started_at":"then","finished_at":"now"
    })).unwrap();
    (evaluation, report)
}

#[test]
fn partial_or_mislabelled_reports_never_claim_qualification() {
    let (evaluation, report) = fixture();
    validate_report(&evaluation, &report).unwrap();
    assert!(super::super::qualification_from_evaluation(&evaluation, report.clone()).is_err());
    for (pointer, value) in [
        ("/scope", json!({"kind":"full_qualification"})),
        ("/gate_passed", json!(true)),
        ("/candidate_safe", json!(true)),
        ("/provider", json!("replay")),
        ("/diagnostic/passed", json!(false)),
        (
            "/report/resolved_settings/evaluation_scope",
            json!({"kind":"full_qualification"}),
        ),
        ("/report/results/0/fixture", json!("failing-baseline")),
        ("/report/results/0/attempt", json!(2)),
        ("/report/results", json!([])),
    ] {
        let mut changed = report.clone();
        *changed.pointer_mut(pointer).unwrap() = value;
        assert!(validate_report(&evaluation, &changed).is_err(), "{pointer}");
    }
    let mut duplicate = report.clone();
    duplicate["report"]["results"]
        .as_array_mut()
        .unwrap()
        .push(report["report"]["results"][0].clone());
    assert!(validate_report(&evaluation, &duplicate).is_err());
    let mut failed = report.clone();
    failed["report"]["results"][0]["passed"] = json!(false);
    failed["diagnostic"]["passed"] = json!(false);
    validate_report(&evaluation, &failed).unwrap();
    let mut legacy = evaluation.clone();
    legacy.scope = InferenceEvaluationScope::FullQualification {};
    validate_report(&legacy, &json!({"gate_passed":true})).unwrap();
    assert!(validate_report(&legacy, &report).is_err());
}

#[test]
fn common_control_receives_only_complete_public_inputs_on_the_same_runtime() {
    let (original, _) = fixture();
    let scope = InferenceEvaluationScope::Diagnostic {
        case_ids: vec!["acceptance-boundary".into()],
        reference_evaluation_id: Some(original.id.clone()),
    };
    let inputs = retained_inputs(
        &original,
        &scope,
        "planner-v2",
        "runtime",
        &original.resolved_binding,
    )
    .unwrap();
    assert_eq!(
        inputs["acceptance-boundary"]["document"]["evidence"]["receipt"],
        "observed"
    );
    assert!(!inputs.to_string().contains("model_output"));
    for field in [
        "status",
        "runtime_revision",
        "suite_hash",
        "scope",
        "report_hash",
    ] {
        let mut value = serde_json::to_value(&original).unwrap();
        value[field] = if field == "scope" {
            json!({"kind":"full_qualification"})
        } else {
            json!("changed")
        };
        let changed = serde_json::from_value(value).unwrap();
        assert!(
            retained_inputs(
                &changed,
                &scope,
                "planner-v2",
                "runtime",
                &original.resolved_binding
            )
            .is_err(),
            "{field}"
        );
    }
    for field in [
        "tool_schema_hash",
        "context_policy_hash",
        "profile_budget_hash",
        "prompt_version",
    ] {
        let mut value = serde_json::to_value(&original.resolved_binding).unwrap();
        value[field] = json!("changed");
        let binding = serde_json::from_value(value).unwrap();
        // Prompt version is checked separately from the optional stage record.
        assert!(
            retained_inputs(&original, &scope, "planner-v2", "runtime", &binding).is_err(),
            "{field}"
        );
    }
    for (pointer, value) in [
        (
            "/report/results/0/stage_submission/measurement_input/retention",
            json!("redacted"),
        ),
        (
            "/report/results/0/stage_submission/measurement_input/document/task",
            json!("altered"),
        ),
        ("/report/results/0/workspace_hash", Value::Null),
    ] {
        let mut changed = original.clone();
        *changed
            .report
            .as_mut()
            .unwrap()
            .pointer_mut(pointer)
            .unwrap() = value;
        changed.report_hash =
            Some(canonical_json_sha256(changed.report.as_ref().unwrap()).unwrap());
        assert!(
            retained_inputs(
                &changed,
                &scope,
                "planner-v2",
                "runtime",
                &original.resolved_binding
            )
            .is_err(),
            "{pointer}"
        );
    }
}

#[test]
fn common_control_reads_source_identity_from_retained_native_evaluator_report() {
    // Real gateway output crosses the producer/consumer boundary that the
    // hand-written fixture above cannot validate by itself.
    let receipt: Value = serde_json::from_str(include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../planning/evidence/autonomous-sdlc/ASTRA-M04-A2D05F1-ONBOARDING-RESULT.json"
    )))
    .unwrap();
    let original: StoredInferenceEvaluation =
        serde_json::from_value(receipt["evaluation"].clone()).unwrap();
    let scope = InferenceEvaluationScope::Diagnostic {
        case_ids: vec!["python-contract".into()],
        reference_evaluation_id: Some(original.id.clone()),
    };
    let row = &original.report.as_ref().unwrap()["report"]["results"][0];
    assert_eq!(
        row["source_sha"],
        "656be973d8d36f53b7a34018861dc59844c4164f"
    );
    assert!(row.get("base_sha").is_none());
    let current_suite_hash = inference_qualification_suite_hash(&original.suite_id).unwrap();
    assert_ne!(original.suite_hash, current_suite_hash);
    assert!(
        retained_inputs(
            &original,
            &scope,
            &original.suite_id,
            &original.runtime_revision,
            &original.resolved_binding,
        )
        .is_err(),
        "the historical native report cannot be a current-suite control"
    );

    // Isolate the source-field reader regression with a test-only request wrapper.
    // The native rows/input hashes remain verbatim. This is not a requalified or
    // rescored report; the untouched historical request was rejected above.
    let mut reader_fixture = original.clone();
    reader_fixture.suite_hash = current_suite_hash;
    let inputs = retained_inputs(
        &reader_fixture,
        &scope,
        &original.suite_id,
        &original.runtime_revision,
        &original.resolved_binding,
    )
    .unwrap();
    let reference = &inputs["python-contract"];
    assert_eq!(reference["base_sha"], row["source_sha"]);
    assert_eq!(reference["workspace_hash"], row["workspace_hash"]);
    assert_eq!(
        reference["document"],
        row["stage_submission"]["measurement_input"]["document"]
    );
    assert_eq!(
        reference["input_hash"],
        row["stage_submission"]["measurement_input"]["raw_content_sha256"]
    );
    assert_eq!(reference.as_object().unwrap().len(), 4);

    // A legacy-looking alias must not fill in missing native source evidence.
    let mut missing_source = reader_fixture.clone();
    let changed_row = &mut missing_source.report.as_mut().unwrap()["report"]["results"][0];
    changed_row["base_sha"] = changed_row["source_sha"].clone();
    changed_row.as_object_mut().unwrap().remove("source_sha");
    missing_source.report_hash =
        Some(canonical_json_sha256(missing_source.report.as_ref().unwrap()).unwrap());
    let error = retained_inputs(
        &missing_source,
        &scope,
        &original.suite_id,
        &original.runtime_revision,
        &original.resolved_binding,
    )
    .unwrap_err();
    assert!(error.message.contains("source_sha"));
}
