use super::{fresh_pass, latest_verification, policy_identity, select_policy};
use pharness_core::{InferencePolicyRef, InferenceRegistry, StageInferencePolicyRevision};
use pharness_store::StoredInferenceTargetVerification;
use serde_json::json;

fn registry() -> InferenceRegistry {
    let mut registry: InferenceRegistry = serde_json::from_str(include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../deploy/helm/pharness/files/inference-registry.json"
    )))
    .unwrap();
    registry.finalize_hashes().unwrap();
    registry
}

fn passed(
    policy: &StageInferencePolicyRevision,
    registry_hash: &str,
) -> StoredInferenceTargetVerification {
    serde_json::from_value(json!({
        "id":"verify_one","target_id":policy.target.target_id,"target_revision":policy.target.revision,
        "target_hash":policy.target_hash,"status":"passed","reachability":"reachable",
        "model_visible":true,"streaming_compatible":true,"tool_compatible":true,
        "observed_capabilities":{"policy":policy_identity(policy),"registry_hash":registry_hash,"runtime_revision":"runtime",
            "protocol_calibration":{"passed":30,"required":30}},
        "sanitized_failure":null,"actor":"operator","reason":"test","config_hash":registry_hash,
        "created_at":"100","expires_at":"1000"
    })).unwrap()
}

#[test]
fn shared_target_requires_exact_policy_instead_of_first_registry_entry() {
    let registry = registry();
    let planner = registry.policy("planner-kimi-k3-v2", "v1").unwrap();
    assert!(select_policy(&registry, "fireworks-kimi-k3", "v1", None).is_err());
    let reference = InferencePolicyRef {
        policy_id: planner.policy_id.clone(),
        revision: planner.revision.clone(),
    };
    let selected = select_policy(&registry, "fireworks-kimi-k3", "v1", Some(&reference)).unwrap();
    assert_eq!(selected, planner);
    assert_eq!(
        selected.reasoning.effort,
        Some(pharness_core::ReasoningEffort::High)
    );
    assert!(select_policy(&registry, "fireworks-minimax-m3", "v1", Some(&reference)).is_err());
    assert!(select_policy(
        &registry,
        "fireworks-kimi-k3",
        "m04-control-v1",
        Some(&reference)
    )
    .is_err());
    let missing = InferencePolicyRef {
        revision: "missing".into(),
        ..reference
    };
    assert!(select_policy(&registry, "fireworks-kimi-k3", "v1", Some(&missing)).is_err());
}

#[test]
fn unambiguous_target_keeps_existing_request_compatibility() {
    let mut registry = registry();
    registry
        .policies
        .retain(|policy| policy.policy_id == "planner-kimi-k3-v2");
    assert_eq!(
        select_policy(&registry, "fireworks-kimi-k3", "v1", None)
            .unwrap()
            .policy_id,
        "planner-kimi-k3-v2"
    );
    assert!(select_policy(&registry, "missing", "v1", None).is_err());
}

#[test]
fn target_pass_and_other_generation_policy_cannot_qualify_planner() {
    let registry = registry();
    let planner = registry.policy("planner-kimi-k3-v2", "v1").unwrap();
    let onboarding = registry.policy("onboarding-kimi-k3-v2", "v1").unwrap();
    let exact = passed(planner, &registry.config_hash);
    assert!(latest_verification(
        vec![exact.clone()],
        planner,
        &registry.config_hash,
        "runtime"
    )
    .is_some());
    assert!(latest_verification(
        vec![passed(onboarding, &registry.config_hash)],
        planner,
        &registry.config_hash,
        "runtime"
    )
    .is_none());
    let mut legacy = exact.clone();
    legacy
        .observed_capabilities
        .as_object_mut()
        .unwrap()
        .remove("policy");
    assert!(latest_verification(vec![legacy], planner, &registry.config_hash, "runtime").is_none());
    for field in ["policy_id", "revision", "policy_hash"] {
        let mut changed = exact.clone();
        changed.observed_capabilities["policy"][field] = json!("changed");
        assert!(
            latest_verification(vec![changed], planner, &registry.config_hash, "runtime").is_none(),
            "{field}"
        );
    }
    for mutate in [
        |v: &mut StoredInferenceTargetVerification| v.target_hash = "changed".into(),
        |v: &mut StoredInferenceTargetVerification| v.target_revision = "changed".into(),
        |v: &mut StoredInferenceTargetVerification| v.config_hash = "changed".into(),
        |v: &mut StoredInferenceTargetVerification| {
            v.observed_capabilities["registry_hash"] = json!("changed")
        },
        |v: &mut StoredInferenceTargetVerification| {
            v.observed_capabilities["runtime_revision"] = json!("changed")
        },
    ] {
        let mut changed = exact.clone();
        mutate(&mut changed);
        assert!(
            latest_verification(vec![changed], planner, &registry.config_hash, "runtime").is_none()
        );
    }
}

#[test]
fn newer_failure_overrides_old_pass_and_stale_or_partial_checks_block() {
    let registry = registry();
    let policy = registry.policy("planner-kimi-k3-v2", "v1").unwrap();
    let exact = passed(policy, &registry.config_hash);
    assert!(fresh_pass(&exact, 999));
    assert!(!fresh_pass(&exact, 1000));
    let mut failed = exact.clone();
    failed.status = "failed".into();
    assert!(!fresh_pass(
        &latest_verification(
            vec![failed, exact.clone()],
            policy,
            &registry.config_hash,
            "runtime"
        )
        .unwrap(),
        500
    ));
    for mutate in [
        |v: &mut StoredInferenceTargetVerification| {
            v.observed_capabilities["protocol_calibration"]["passed"] = json!(29)
        },
        |v: &mut StoredInferenceTargetVerification| {
            v.observed_capabilities["protocol_calibration"]["required"] = json!(29)
        },
        |v: &mut StoredInferenceTargetVerification| v.model_visible = false,
        |v: &mut StoredInferenceTargetVerification| v.streaming_compatible = false,
        |v: &mut StoredInferenceTargetVerification| v.tool_compatible = false,
        |v: &mut StoredInferenceTargetVerification| {
            v.sanitized_failure = Some("contradiction".into())
        },
        |v: &mut StoredInferenceTargetVerification| v.expires_at = "invalid".into(),
    ] {
        let mut changed = exact.clone();
        mutate(&mut changed);
        assert!(!fresh_pass(&changed, 500));
    }
}
