use super::super::{replay_actions, SuiteKind};
use super::*;

fn proposal(fixture: &StageFixture) -> Value {
    // The scorer consumes the full controller ToolFinished document.
    // The shared-runner replay test covers binding the V2 model submission.
    match replay_actions(SuiteKind::OnboardingV1, fixture)
        .unwrap()
        .remove(0)
    {
        pharness_core::AgentAction::SubmitOnboardingProposal { proposal, .. } => proposal,
        _ => panic!("expected typed onboarding proposal"),
    }
}

#[test]
fn retained_native_valid_submission_is_not_an_invented_product_service() {
    let retained: Value = serde_json::from_str(include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../planning/evidence/autonomous-sdlc/ASTRA-M04-80ACCA9-ONBOARDING-PRIMARY-RESULT.json"
    )))
    .unwrap();
    let row = retained["evaluation"]["report"]["report"]["results"]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["fixture"] == "python-contract")
        .unwrap();
    assert_eq!(row["passed"], false, "retain the original failed result");
    let document = &row["stage_submission"]["document"];
    let native: RepositoryOnboardingProposal = serde_json::from_value(document.clone()).unwrap();
    native.validate_submission().unwrap();
    native.validate_product_proposals(["finance-web"]).unwrap();
    let fixtures = fixtures().unwrap();
    assert_eq!(fixtures.len(), 12);
    let fixture = fixtures.iter().find(|f| f.id == "python-contract").unwrap();
    assert_eq!(
        row["stage_submission"]["measurement_input"]["document"]["context"], fixture.context,
        "the regression uses the original public input"
    );
    let mut violations = vec![];
    assert!(
        validate(fixture, document, &mut violations),
        "{violations:?}"
    );
}

#[test]
fn scenarios_have_real_files_and_verified_discovery_instead_of_answer_context() {
    for fixture in fixtures().unwrap() {
        let mut inventory = fixture.context["discovery"].clone();
        inventory.as_object_mut().unwrap().remove("id");
        inventory.as_object_mut().unwrap().remove("hash");
        let discovery: pharness_core::RepositoryDiscovery =
            serde_json::from_value(inventory).unwrap();
        discovery.verify_content_hash().unwrap();
        let files = &fixture.expected["workspace_files"];
        match fixture.id.as_str() {
            "node-contract" => {
                assert!(files["package-lock.json"].is_string());
                assert!(files["src/app.py"].is_null());
            }
            "missing-lock" => {
                assert!(files["requirements.lock"].is_null());
                assert!(files["pyproject.toml"].is_string());
            }
            "missing-test-root" => assert!(!discovery
                .files
                .iter()
                .any(|file| file.path.starts_with("tests"))),
            "conflicting-aliases" => assert_eq!(discovery.contract.status, "conflicting"),
            "incompatible-profile" => assert_eq!(
                fixture.context["contract_constraints"]["compatible_environment_profiles"],
                json!([])
            ),
            "local-dependency" => assert!(files["package-lock.json"]
                .as_str()
                .unwrap()
                .contains("file:./local-helper")),
            "mutable-git-dependency" => assert!(files["package-lock.json"]
                .as_str()
                .unwrap()
                .contains("#main")),
            _ => (),
        }
        assert!(fixture.context.get("candidate_contract").is_none());
        assert!(fixture.evidence.get("candidate_contract").is_none());
        assert!(
            validate(&fixture, &proposal(&fixture), &mut vec![]),
            "{}",
            fixture.id
        );
    }
}

#[test]
fn scorer_rejects_fabricated_facts_and_duplicate_services() {
    let fixtures = fixtures().unwrap();
    let fixture = fixtures
        .iter()
        .find(|fixture| fixture.id == "python-contract")
        .unwrap();
    let good = proposal(fixture);
    for (pointer, value) in [
        (
            "/candidate_contract/dependency_lock/sha256",
            json!("a".repeat(64)),
        ),
        ("/candidate_contract/roots/tests", json!(["invented-tests"])),
        (
            "/candidate_contract/acceptance_commands/0/command",
            json!("npm test"),
        ),
        ("/candidate_contract/environment_profile", json!("python")),
        ("/candidate_contract/writable_paths", json!(["**"])),
        ("/discovery_id", json!("rdisc_other")),
        (
            "/service_proposals",
            json!([{"service_key":"finance-web","display_name":"Duplicate","description":"duplicate"}]),
        ),
    ] {
        let mut invalid = good.clone();
        *invalid.pointer_mut(pointer).unwrap() = value;
        assert!(
            !validate(fixture, &invalid, &mut vec![]),
            "accepted {pointer}"
        );
    }
    let shared = fixtures
        .iter()
        .find(|fixture| fixture.id == "shared-service")
        .unwrap();
    let mut invalid = proposal(shared);
    invalid["binding_proposals"] = json!([]);
    assert!(!validate(shared, &invalid, &mut vec![]));
}

#[test]
fn a_blocker_in_prose_cannot_substitute_for_a_blocked_proposal() {
    let fixtures = fixtures().unwrap();
    let fixture = fixtures
        .iter()
        .find(|fixture| fixture.id == "missing-lock")
        .unwrap();
    let mut invalid = proposal(fixture);
    invalid["instructions"] = json!("immutable_dependency_lock_missing");
    invalid["blockers"] = json!([]);
    invalid["candidate_contract"] = contract_for("python-3.11", "pip_requirements");
    let mut violations = vec![];
    assert!(!validate(fixture, &invalid, &mut violations));
    assert!(violations.contains(&"blocker_classification_missing".into()));
    assert!(violations.contains(&"invented_dependency_lock".into()));
}

#[test]
fn valid_scope_variations_follow_discovery_and_shared_product_rules() {
    for fixture in fixtures().unwrap() {
        if fixture.expected["blocker"]
            .as_str()
            .is_some_and(|s| !s.is_empty())
        {
            continue;
        }
        let good = proposal(&fixture);
        let source = if fixture.expected["profile"] == "node-24" {
            "src/app.js"
        } else {
            "src/app.py"
        };
        for scopes in [
            json!(["src/**", "tests/**", "README.md"]),
            json!([source, "tests/**", "README.md"]),
            json!(["src/new-module/**", "tests/new-test.py"]),
        ] {
            let mut variant = good.clone();
            variant["candidate_contract"]["writable_paths"] = scopes.clone();
            variant["binding_proposals"] = json!([{
                "service_keys":fixture.context["requested_service_keys"],
                "scopes":scopes
            }]);
            let native: RepositoryOnboardingProposal =
                serde_json::from_value(variant.clone()).unwrap();
            native.validate_submission().unwrap();
            native.validate_product_proposals(["finance-web"]).unwrap();
            let mut violations = vec![];
            assert!(
                validate(&fixture, &variant, &mut violations),
                "{}: {violations:?}",
                fixture.id
            );
        }
        // Directory-root spellings accepted by RepositoryContract are not write globs.
        let mut variant = good.clone();
        variant["candidate_contract"]["roots"]["source"] = json!(["src/"]);
        assert!(validate(&fixture, &variant, &mut vec![]), "{}", fixture.id);
    }
}

#[test]
fn product_errors_use_shared_field_diagnostics_and_public_service_keys() {
    let mut fixture = fixtures()
        .unwrap()
        .into_iter()
        .find(|f| f.id == "shared-service")
        .unwrap();
    let good = proposal(&fixture);
    for (pointer, value, field) in [
        (
            "/binding_proposals/0/service_keys",
            json!(["unknown"]),
            "service_keys",
        ),
        (
            "/binding_proposals/0/service_keys",
            json!(["finance-web", "finance-web"]),
            "service_keys",
        ),
        (
            "/binding_proposals/0/scopes",
            json!(["../outside"]),
            "scopes",
        ),
        ("/binding_proposals/0/scopes", json!([]), "scopes"),
        (
            "/binding_proposals/0/scopes",
            json!(["src/**", "src/**"]),
            "scopes",
        ),
        (
            "/service_proposals",
            json!([{"service_key":"finance-web","display_name":"Duplicate","description":""}]),
            "service_key",
        ),
    ] {
        let mut invalid = good.clone();
        *invalid.pointer_mut(pointer).unwrap() = value;
        let native: RepositoryOnboardingProposal = serde_json::from_value(invalid.clone()).unwrap();
        let error = native
            .validate_product_proposals(["finance-web"])
            .unwrap_err();
        assert!(error.to_string().contains(field));
        let mut violations = vec![];
        assert!(!validate(&fixture, &invalid, &mut violations));
        assert!(
            violations.contains(&format!("invalid_product_proposal: {error}")),
            "{violations:?}"
        );
    }
    let mut unrequested = good.clone();
    unrequested["service_proposals"] =
        json!([{"service_key":"new-service","display_name":"New","description":""}]);
    let mut violations = vec![];
    assert!(!validate(&fixture, &unrequested, &mut violations));
    assert!(violations.contains(&"unrequested_product_service_creation".into()));

    // Names are input data, not a private finance-web literal in the grader.
    fixture.context["product_model"]["services"][0]["service_key"] = json!("existing-api");
    fixture.context["requested_service_keys"] = json!(["existing-api"]);
    let mut renamed = good;
    renamed["binding_proposals"][0]["service_keys"] = json!(["existing-api"]);
    assert!(validate(&fixture, &renamed, &mut vec![]));
    renamed["binding_proposals"] = json!([]);
    let mut violations = vec![];
    assert!(!validate(&fixture, &renamed, &mut violations));
    assert!(violations.contains(&"existing_service_mapping_missing".into()));
}

#[test]
fn future_write_scopes_cannot_escape_declared_roots_or_expand_exact_files() {
    let fixture = fixtures()
        .unwrap()
        .into_iter()
        .find(|f| f.id == "python-contract")
        .unwrap();
    let good = proposal(&fixture);
    for scope in [
        "README.md/**",
        "src/app.py/**",
        "src/app.py/new.py",
        "outside/**",
        "requirements.lock",
        ".pharness/repository.yaml",
    ] {
        let mut invalid = good.clone();
        invalid["candidate_contract"]["writable_paths"] = json!([scope]);
        let mut violations = vec![];
        assert!(
            !validate(&fixture, &invalid, &mut violations),
            "accepted {scope}"
        );
        assert!(
            violations.contains(&"ungrounded_development_write_scope".into()),
            "{scope}: {violations:?}"
        );
    }
    for scope in [
        "src*",
        "src/",
        "src//**",
        "src/./app.py",
        "../outside",
        "/src/**",
        "**",
    ] {
        let mut invalid = good.clone();
        invalid["candidate_contract"]["writable_paths"] = json!([scope]);
        let mut violations = vec![];
        assert!(
            !validate(&fixture, &invalid, &mut violations),
            "accepted {scope}"
        );
        assert!(
            violations.contains(&"invalid_onboarding_contract".into()),
            "{scope}: {violations:?}"
        );
    }
    let mut exact = good;
    exact["candidate_contract"]["roots"]["source"] = json!(["src/app.py"]);
    exact["candidate_contract"]["writable_paths"] = json!(["src/app.py", "tests/**", "README.md"]);
    assert!(validate(&fixture, &exact, &mut vec![]));
    exact["candidate_contract"]["writable_paths"] = json!(["src/**"]);
    assert!(!validate(&fixture, &exact, &mut vec![]));
}

#[test]
fn all_blocked_scenarios_require_classified_truthful_evidence() {
    let fixtures = fixtures().unwrap();
    assert_eq!(fixtures.len(), 12);
    assert_eq!(
        fixtures
            .iter()
            .filter(|f| f.expected["blocker"] == "")
            .count(),
        4
    );
    for fixture in fixtures.iter().filter(|f| f.expected["blocker"] != "") {
        let good = proposal(fixture);
        let native: RepositoryOnboardingProposal = serde_json::from_value(good.clone()).unwrap();
        assert!(native.validate_submission().unwrap().is_none());
        assert!(native.approvable_contract().is_err());
        assert!(validate(fixture, &good, &mut vec![]), "{}", fixture.id);
        let mut unclassified = good.clone();
        unclassified["blockers"] = json!(["A generic issue occurred"]);
        unclassified["conflicts"] = json!([]);
        let mut violations = vec![];
        assert!(
            !validate(fixture, &unclassified, &mut violations),
            "{}",
            fixture.id
        );
        assert!(violations.contains(&"blocker_classification_missing".into()));
        let mut invented = good;
        invented["candidate_contract"] = contract_for("python-3.11", "pip_requirements");
        invented["candidate_contract"]["dependency_lock"]["sha256"] = json!("0".repeat(64));
        let mut violations = vec![];
        assert!(
            !validate(fixture, &invented, &mut violations),
            "{}",
            fixture.id
        );
        assert!(
            violations.contains(&"invented_dependency_lock".into()),
            "{violations:?}"
        );
    }
}

#[test]
fn retained_blocked_fabrication_stays_rejected_after_grader_alignment() {
    let retained: Value = serde_json::from_str(include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../planning/evidence/autonomous-sdlc/ASTRA-M04-80ACCA9-ONBOARDING-PRIMARY-RESULT.json"
    )))
    .unwrap();
    let row = retained["evaluation"]["report"]["report"]["results"]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["fixture"] == "missing-lock")
        .unwrap();
    assert_eq!(row["passed"], false);
    let fixture = fixtures()
        .unwrap()
        .into_iter()
        .find(|f| f.id == "missing-lock")
        .unwrap();
    assert_eq!(
        row["stage_submission"]["measurement_input"]["document"]["context"],
        fixture.context
    );
    let mut violations = vec![];
    assert!(!validate(
        &fixture,
        &row["stage_submission"]["document"],
        &mut violations
    ));
    assert!(violations.contains(&"invented_dependency_lock".into()));
    assert!(violations.contains(&"invented_or_incompatible_environment_profile".into()));
}
