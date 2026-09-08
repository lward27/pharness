use super::*;
use pharness_core::{diagnostic_cases, InferenceEvaluationScope};

#[test]
fn diagnostic_control_policies_preserve_role_limits_and_existing_defaults() {
    let mut registry: pharness_core::InferenceRegistry = serde_json::from_str(include_str!(
        "../../../../deploy/helm/pharness/files/inference-registry.json"
    ))
    .unwrap();
    registry.finalize_hashes().unwrap();
    registry.validate().unwrap();
    assert!(!serde_json::to_string(&registry.defaults)
        .unwrap()
        .contains("m04-control"));
    for (role, original) in [
        ("onboarding", "onboarding-minimax-m3-v2"),
        ("planner", "planner-kimi-k3-v2"),
        ("test-diagnosis", "test-diagnosis-nemotron-v2"),
        ("verifier", "verifier-glm-5p3-v2"),
    ] {
        let control = registry
            .policy(&format!("m04-control-{role}-kimi-k3-v2"), "v1")
            .unwrap();
        let primary = registry.policy(original, "v1").unwrap();
        let mut left = serde_json::to_value(control).unwrap();
        let mut right = serde_json::to_value(primary).unwrap();
        for key in [
            "policy_id",
            "display_name",
            "target",
            "target_hash",
            "policy_hash",
        ] {
            left.as_object_mut().unwrap().remove(key);
            right.as_object_mut().unwrap().remove(key);
        }
        assert_eq!(
            left, right,
            "{role}: comparison changed more than the named model"
        );
        assert_eq!(
            registry
                .target(&control.target.target_id, &control.target.revision)
                .unwrap()
                .upstream_model,
            "accounts/fireworks/models/kimi-k3"
        );
    }
}

#[tokio::test]
async fn declared_canaries_run_only_requested_cases_and_cannot_qualify() {
    for suite in [
        "onboarding-v2",
        "planner-v2",
        "test-diagnosis-v2",
        "verifier-v2",
    ] {
        let scope = InferenceEvaluationScope::Diagnostic {
            case_ids: diagnostic_cases(suite)
                .iter()
                .map(|s| s.to_string())
                .collect(),
            reference_evaluation_id: None,
        };
        let report = run_scoped(suite, Provider::Replay, 1, None, None, scope.clone())
            .await
            .unwrap();
        assert_eq!(report.results.len(), scope.case_ids().unwrap().len());
        assert!(report.results.iter().all(|r| r.passed));
        let evidence = crate::qualification_evidence(&report);
        assert_eq!(evidence["scope"], serde_json::to_value(scope).unwrap());
        assert_eq!(evidence["diagnostic"]["passed"], true);
        assert_eq!(evidence["gate_passed"], false);
        assert_eq!(evidence["candidate_safe"], false);
        for result in &report.results {
            let retained = &result.stage_submission.as_ref().unwrap()["measurement_input"];
            assert_eq!(retained["retention"], "complete");
            assert_eq!(
                retained["raw_content_sha256"],
                canonical_json_sha256(&retained["document"]).unwrap()
            );
            assert!(result.workspace_hash.is_some());
            // Exercise the serialized report -> reference -> reconstructed
            // workspace boundary, rather than only prepared-fixture fields.
            let native = serde_json::to_value(result).unwrap();
            assert!(native["source_sha"].is_string());
            assert!(native.get("base_sha").is_none());
            let kind = SuiteKind::parse(suite).unwrap();
            let mut fixture = fixtures(kind)
                .unwrap()
                .into_iter()
                .find(|fixture| fixture.id == result.fixture)
                .unwrap();
            fixture.expected["diagnostic_input_reference"] = json!({
                "document":retained["document"],
                "input_hash":retained["raw_content_sha256"],
                "workspace_hash":native["workspace_hash"],
                "base_sha":native["source_sha"]
            });
            let (reconstructed, root) = prepare_case(kind, &fixture, 1).unwrap();
            assert_eq!(reconstructed.evidence, retained["document"]["evidence"]);
            fs::remove_dir_all(root).unwrap();
        }
    }
}

#[test]
fn control_reuses_public_inputs_but_rejects_changed_source_or_context() {
    for suite in [
        SuiteKind::OnboardingV2,
        SuiteKind::PlannerV2,
        SuiteKind::TestDiagnosisV2,
        SuiteKind::VerifierV2,
    ] {
        let mut fixture = fixtures(suite)
            .unwrap()
            .into_iter()
            .find(|f| f.id == diagnostic_cases(suite.suite_id())[0])
            .unwrap();
        let (original, root) = prepare_case(suite, &fixture, 1).unwrap();
        let document =
            json!({"task":original.task,"context":original.context,"evidence":original.evidence});
        fixture.expected["diagnostic_input_reference"] = json!({"document":document,"input_hash":canonical_json_sha256(&document).unwrap(),"workspace_hash":original.expected["prepared_source_hash"],"base_sha":original.expected["prepared_base_sha"]});
        let (control, second_root) = prepare_case(suite, &fixture, 1).unwrap();
        assert_eq!(original.evidence, control.evidence);
        assert_eq!(original.context, control.context);
        assert_eq!(
            original.expected["prepared_source_hash"],
            control.expected["prepared_source_hash"]
        );
        fs::remove_dir_all(root).unwrap();
        fs::remove_dir_all(second_root).unwrap();
        for field in ["workspace_hash", "base_sha", "input_hash", "document"] {
            let mut changed = fixture.clone();
            changed.expected["diagnostic_input_reference"][field] = json!("changed");
            assert!(
                prepare_case(suite, &changed, 1).is_err(),
                "{suite:?}: {field}"
            );
        }
        let mut changed = fixture.clone();
        changed.context["intent"] = json!("a different request");
        assert!(prepare_case(suite, &changed, 1).is_err());
    }
}
