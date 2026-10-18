use super::*;
use pharness_core::hosted_sdlc::{
    gitops_patch::update_kustomization_image,
    staging::{AUTHORITY_SCHEMA, PLAN_SCHEMA},
};
use serde_json::json;

#[test]
fn writer_rejects_missing_expired_rebound_or_expanded_baseline_admission() {
    let a:HostedStagingAuthority=serde_json::from_value(json!({"schema_version":AUTHORITY_SCHEMA,"work_item_id":"work_fixture","operation_id":"operation_fixture",
        "execution_id":"execution_fixture","pipeline_intent_id":"pipeline_fixture","deployment_intent_id":"deployment_fixture",
        "gitops_change_set_id":"gitops_fixture","workflow_policy_hash":format!("sha256:{}","a".repeat(64)),
        "build_evidence_hash":format!("sha256:{}","b".repeat(64)),"application_repository":"https://github.com/lward27/yfinance_wrapper.git",
        "source_commit_sha":"c".repeat(40),"image_digest":format!("sha256:{}","d".repeat(64)),"created_at_ms":1000,"expires_at_ms":3601000})).unwrap();
    let source=format!("apiVersion: kustomize.config.k8s.io/v1beta1\nkind: Kustomization\nnamespace: apps-staging\nimages:\n  - name: registry.lucas.engineering/yfinance_wrapper\n    digest: sha256:{}\n","e".repeat(64));
    let p = StagingGitOpsPlan {
        schema_version: PLAN_SCHEMA.into(),
        authority_hash: a.material_hash().unwrap(),
        base_commit_sha: "a".repeat(40),
        original_blob_sha: "b".repeat(40),
        updated_content: update_kustomization_image(
            &source,
            a.coordinates().unwrap().1,
            &a.image_ref().unwrap(),
        )
        .unwrap(),
        original_content: source,
    };
    let value = json!({"admitted":true,"authority_hash":a.material_hash().unwrap(),"plan_hash":p.material_hash().unwrap(),"baseline":{"baseline_artifact_id":"staging_baseline_result_execution_fixture","identity_artifact_id":"staging_baseline_admission_identity_execution_fixture","baseline_sha256":format!("sha256:{}","1".repeat(64)),"identity_sha256":format!("sha256:{}","2".repeat(64)),"admission_expires_at_ms":460_000}});
    validate(&value, &a, &p, 450_000).unwrap();
    for (pointer, bad) in [
        ("/baseline", Value::Null),
        ("/baseline/admission_expires_at_ms", json!(450_000)),
        ("/baseline/admission_expires_at_ms", json!(510_001)),
        ("/baseline/admission_expires_at_ms", json!(1_801_000)),
        ("/baseline/identity_sha256", json!("unverified")),
        ("/baseline/baseline_artifact_id", json!("another-window")),
        (
            "/baseline/identity_artifact_id",
            json!("another-native-read"),
        ),
        ("/authority_hash", json!("changed")),
        ("/plan_hash", json!("changed")),
        ("/admitted", json!(false)),
    ] {
        let mut changed = value.clone();
        *changed.pointer_mut(pointer).unwrap() = bad;
        assert!(validate(&changed, &a, &p, 450_000).is_err(), "{pointer}");
    }
    assert!(
        validate(&value, &a, &p, 460_000).is_err(),
        "expiry is checked again immediately before the one POST"
    );
}
