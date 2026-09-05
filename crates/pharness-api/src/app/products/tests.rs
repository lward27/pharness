use super::model::{normalize_key, validate_repository_binding_scope};
use super::onboarding_policy::{
    onboarding_environment_profile_descriptors, onboarding_environment_profile_ids,
    onboarding_patch_paths, validate_binding_scope,
};
use super::readiness::readiness_current_state;
use super::registration::parse_github_repository_url;

use pharness_core::{EnvironmentProfile, EnvironmentProfileLimits, PreparationStrategy};
use serde_json::json;

#[test]
fn product_keys_are_stable_and_bounded() {
    assert_eq!(normalize_key(" Orion Platform ").unwrap(), "orion-platform");
    assert_eq!(normalize_key("API / Core").unwrap(), "api-core");
    assert!(normalize_key("---").is_err());
    assert!(normalize_key(&"a".repeat(65)).is_err());
}

#[test]
fn onboarding_binding_scopes_are_repository_relative_and_normalized() {
    assert!(validate_binding_scope("**").is_ok());
    assert!(validate_binding_scope("src/**").is_ok());
    for invalid in ["", "/src/**", "../src/**", "src/../tests/**", "src\\**"] {
        assert!(
            validate_binding_scope(invalid).is_err(),
            "accepted {invalid}"
        );
    }
}

#[test]
fn typed_product_scopes_accept_bounded_globs_and_reject_escapes() {
    for (path, role) in [
        ("**", "source"),
        ("charts/yfinance-wrapper/**", "delivery"),
        (
            "charts/root-app/templates/yfinance-wrapper.yaml",
            "product_integration",
        ),
        ("charts/root-app/templates/*.yaml", "product_integration"),
    ] {
        assert!(
            validate_repository_binding_scope(path, role).is_ok(),
            "rejected {path}"
        );
    }
    for path in [
        "",
        "/charts/**",
        "../charts/**",
        "charts/../secret",
        "charts\\**",
    ] {
        assert!(validate_repository_binding_scope(path, "delivery").is_err());
    }
    assert!(validate_repository_binding_scope("charts/**", "cluster_owner").is_err());
}

#[test]
fn onboarding_product_proposals_reject_unknown_fields() {
    let proposal = json!({
        "schema_version":pharness_core::ONBOARDING_PROPOSAL_SCHEMA,
        "discovery_id":"rdisc_test",
        "discovery_hash":"sha256:discovery",
        "candidate_contract":{},
        "instructions":"",
        "service_proposals":[{
            "service_key":"api",
            "display_name":"API",
            "description":"API service",
            "unreviewed":true
        }],
        "binding_proposals":[],
        "assumptions":[],
        "conflicts":[],
        "blockers":[],
        "readiness_forecast":{}
    });
    assert!(
        serde_json::from_value::<pharness_core::RepositoryOnboardingProposal>(proposal).is_err()
    );
}

#[test]
fn onboarding_context_lists_only_exact_active_environment_profile_ids() {
    assert_eq!(
        onboarding_environment_profile_ids([
            ("python-3.12", true),
            ("python", false),
            ("python-3.11", true),
        ]),
        vec!["python-3.11".to_string(), "python-3.12".to_string()]
    );
}

#[test]
fn onboarding_descriptors_require_matching_repository_and_discovered_lock() {
    let profile = EnvironmentProfile {
        id: "node-24".into(),
        active: true,
        image: format!("registry.example/node@sha256:{}", "a".repeat(64)),
        revision: "b".repeat(40),
        platform: "linux/amd64".into(),
        required_executables: vec![
            "pharness-worker".into(),
            "git".into(),
            "node".into(),
            "npm".into(),
        ],
        preparation_strategy: PreparationStrategy::NodeNpmCi,
        service_account: "pharness-node-runner".into(),
        repository_allowlist: vec!["https://github.com/example/frontend.git".into()],
        limits: EnvironmentProfileLimits {
            cpu: "2".into(),
            memory: "2Gi".into(),
            ephemeral_storage: "4Gi".into(),
        },
    };
    let npm_discovery =
        json!({"dependency_candidates":[{"kind":"npm_package_lock","path":"package-lock.json"}]});
    let descriptors = onboarding_environment_profile_descriptors(
        std::slice::from_ref(&profile),
        "https://github.com/example/frontend.git",
        &npm_discovery,
    );
    assert_eq!(descriptors.len(), 1);
    assert_eq!(descriptors[0]["runtime_kind"], "node");
    assert_eq!(descriptors[0]["lifecycle_scripts"], "denied");
    let pip_discovery =
        json!({"dependency_candidates":[{"kind":"pip_requirements","path":"requirements.lock"}]});
    assert!(onboarding_environment_profile_descriptors(
        std::slice::from_ref(&profile),
        "https://github.com/example/frontend.git",
        &pip_discovery,
    )
    .is_empty());
    assert!(onboarding_environment_profile_descriptors(
        &[profile],
        "https://github.com/example/other.git",
        &npm_discovery,
    )
    .is_empty());
}

#[test]
fn github_registration_urls_are_canonical_and_credential_free() {
    assert_eq!(
        parse_github_repository_url("https://github.com/Example/repo.git").unwrap(),
        "Example/repo"
    );
    assert!(parse_github_repository_url("git@github.com:Example/repo.git").is_err());
    assert!(parse_github_repository_url("https://token@github.com/Example/repo.git").is_err());
    assert!(parse_github_repository_url("https://github.com/Example/repo.git?ref=main").is_err());
    assert!(parse_github_repository_url("https://github.com/Example/repo/extra").is_err());
}

#[test]
fn onboarding_patch_paths_are_controller_bounded() {
    let patch = "diff --git a/.pharness/project.yaml b/.pharness/project.yaml\n--- a/.pharness/project.yaml\n+++ /dev/null\ndiff --git a/.pharness/repository.yaml b/.pharness/repository.yaml\n--- /dev/null\n+++ b/.pharness/repository.yaml\n";
    assert_eq!(
        onboarding_patch_paths(patch).unwrap(),
        vec![
            ".pharness/project.yaml".to_string(),
            ".pharness/repository.yaml".to_string()
        ]
    );
    let escaped = "diff --git a/src/main.rs b/src/main.rs\n";
    assert!(onboarding_patch_paths(escaped).is_err());
    let rename = "diff --git a/.pharness/project.yaml b/.pharness/repository.yaml\n";
    assert_eq!(
        onboarding_patch_paths(rename).unwrap(),
        vec![
            ".pharness/project.yaml".to_string(),
            ".pharness/repository.yaml".to_string()
        ]
    );
    let escaped_rename = "diff --git a/.pharness/project.yaml b/src/repository.yaml\n";
    assert!(onboarding_patch_paths(escaped_rename).is_err());
}

#[test]
fn repository_readiness_projects_missing_stale_and_current_without_client_inference() {
    assert_eq!(
        readiness_current_state(false, &["assessment_missing".into()]),
        (
            false,
            "missing",
            vec![json!({
                "code":"assessment_missing",
                "summary":"no immutable readiness assessment exists for the exact source commit",
            })]
        )
    );
    assert_eq!(
        readiness_current_state(true, &["environment_profile_tuple_changed".into()]),
        (
            false,
            "stale",
            vec![json!({
                "code":"environment_profile_tuple_changed",
                "summary":"the EnvironmentProfile revision or immutable runner digest changed",
            })]
        )
    );
    assert_eq!(
        readiness_current_state(true, &[]),
        (true, "ready", Vec::new())
    );
}
