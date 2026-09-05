use pharness_core::hosted_sdlc::{HostedWorkflowConfig, HostedWorkflowPolicySnapshot};
use serde_json::{json, Value};

fn fixture() -> Value {
    serde_json::from_str(include_str!("fixtures/hosted-workflow.json")).unwrap()
}

#[test]
fn policy_rejects_expanded_authority_and_incompatible_contracts() {
    let original: HostedWorkflowPolicySnapshot = serde_json::from_value(fixture()).unwrap();
    original.validate().unwrap();
    for (pointer, replacement) in [
        (
            "/schema_version",
            json!("pharness.dev/hosted-workflow/future"),
        ),
        ("/production_approval", json!("after_gitops_merge")),
        ("/automatic_actions/0", json!("production_merge")),
        ("/rollback", json!("unlimited")),
        ("/max_attempts", json!(3)),
        ("/builder_budget/hard_tokens", json!(1_000_001)),
        ("/builder_budget/active_execution_seconds", json!(3_601)),
        ("/staging_contract/target_namespace", json!("apps-prod")),
        ("/production_contract/status", json!("draft")),
        ("/stage_inference/test/mode", json!("model_claim")),
        ("/delivery_binding_hash", json!("sha256:changed")),
    ] {
        let mut value = fixture();
        *value.pointer_mut(pointer).unwrap() = replacement;
        let accepted = serde_json::from_value::<HostedWorkflowPolicySnapshot>(value)
            .ok()
            .is_some_and(|policy| policy.validate().is_ok());
        assert!(!accepted, "expanded authority was accepted at {pointer}");
    }
}

#[test]
fn bindings_are_finite_and_do_not_fall_back_when_enabled() {
    HostedWorkflowConfig::default().validate().unwrap();
    assert!(HostedWorkflowConfig {
        enabled: true,
        bindings: vec![]
    }
    .validate()
    .is_err());
    let binding = fixture()["delivery_binding"].clone();
    let config: HostedWorkflowConfig =
        serde_json::from_value(json!({"enabled":true,"bindings":[binding.clone()]})).unwrap();
    config.validate().unwrap();
    for (field, replacement) in [
        (
            "cluster_context",
            json!("admin@lucas-engineering-agent-homelab"),
        ),
        (
            "source_repo",
            json!("https://github.com/example/repo.git?token=not-a-token"),
        ),
        (
            "image_name",
            json!("registry.lucas.engineering/test:latest"),
        ),
        ("gitops_ref", json!("*")),
    ] {
        let mut changed = binding.clone();
        changed[field] = replacement;
        let config: HostedWorkflowConfig =
            serde_json::from_value(json!({"enabled":true,"bindings":[changed]})).unwrap();
        assert!(config.validate().is_err(), "{field}");
    }
    let duplicated: HostedWorkflowConfig =
        serde_json::from_value(json!({"enabled":true,"bindings":[binding.clone(),binding]}))
            .unwrap();
    assert!(duplicated.validate().is_err());
}
