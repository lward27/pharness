use super::*;

fn authority(frontend: bool) -> HostedStagingAuthority {
    HostedStagingAuthority {
        schema_version: AUTHORITY_SCHEMA.into(),
        work_item_id: "work_fixture".into(),
        operation_id: "operation_fixture".into(),
        execution_id: "execution_fixture".into(),
        pipeline_intent_id: "pipeline_fixture".into(),
        deployment_intent_id: "deployment_fixture".into(),
        gitops_change_set_id: "gitops_fixture".into(),
        workflow_policy_hash: format!("sha256:{}", "a".repeat(64)),
        build_evidence_hash: format!("sha256:{}", "b".repeat(64)),
        application_repository: format!(
            "https://github.com/lward27/{}.git",
            if frontend {
                "finance-frontend"
            } else {
                "yfinance_wrapper"
            }
        ),
        source_commit_sha: "c".repeat(40),
        image_digest: format!("sha256:{}", "d".repeat(64)),
        created_at_ms: 1_000,
        expires_at_ms: 3_601_000,
    }
}

fn plan(authority: &HostedStagingAuthority, frontend: bool) -> StagingGitOpsPlan {
    // Unmodified finite staging Kustomizations from GitOps main 491f081e.
    let original = if frontend {
        include_str!("fixtures/frontend.yaml")
    } else {
        include_str!("fixtures/yfinance.yaml")
    };
    StagingGitOpsPlan {
        schema_version: PLAN_SCHEMA.into(),
        authority_hash: authority.material_hash().unwrap(),
        base_commit_sha: "e".repeat(40),
        original_blob_sha: "f".repeat(40),
        original_content: original.into(),
        updated_content: update_kustomization_image(
            original,
            authority.coordinates().unwrap().1,
            &authority.image_ref().unwrap(),
        )
        .unwrap(),
    }
}

#[test]
fn staging_request_is_one_digest_change_on_the_expected_gitops_head() {
    for frontend in [false, true] {
        let a = authority(frontend);
        let p = plan(&a, frontend);
        p.validate(&a).unwrap();
        let request = p.commit_request(&a, 2_000).unwrap();
        let input = &request["variables"]["input"];
        assert_eq!(
            input["branch"],
            json!({"repositoryNameWithOwner":"lward27/lucas_engineering","refName":"main"})
        );
        assert_eq!(input["expectedHeadOid"], p.base_commit_sha);
        let changes = input["fileChanges"].as_object().unwrap();
        assert_eq!(changes.len(), 1);
        let additions = changes["additions"].as_array().unwrap();
        assert_eq!(additions.len(), 1);
        assert_eq!(additions[0]["path"], a.coordinates().unwrap().0);
        assert!(additions[0]["path"]
            .as_str()
            .unwrap()
            .starts_with("charts/finance-staging/"));
        let encoded = additions[0]["contents"].as_str().unwrap();
        assert_eq!(
            STANDARD.decode(encoded).unwrap(),
            p.updated_content.as_bytes()
        );
        let old: Value = serde_yaml::from_str(&p.original_content).unwrap();
        let new: Value = serde_yaml::from_str(&p.updated_content).unwrap();
        let mut expected = old.clone();
        expected["images"][0]["digest"] = json!(a.image_digest);
        assert_eq!(new, expected);
        assert_eq!(
            p.updated_content,
            p.original_content.replace(
                old["images"][0]["digest"].as_str().unwrap(),
                &a.image_digest
            )
        );
        assert_ne!(p.material_hash().unwrap(), a.material_hash().unwrap());
        assert!(input["message"]["body"]
            .as_str()
            .unwrap()
            .contains(&p.material_hash().unwrap()));
        assert!(p.commit_request(&a, a.expires_at_ms).is_err());
        assert!(p.commit_request(&a, 999).is_err());
        p.validate(&a).unwrap(); // Expired plans remain readable for observation.
    }
}

#[test]
fn staging_authority_cannot_switch_application_production_or_execution_limits() {
    let good = authority(false);
    let p = plan(&good, false);
    for (field, value) in [
        ("schema_version", json!("other")),
        ("execution_id", json!("../escape")),
        ("image_digest", json!("latest")),
        ("source_commit_sha", json!("main")),
        (
            "application_repository",
            json!("https://github.com/lward27/lucas_engineering.git"),
        ),
        ("expires_at_ms", json!(3_601_001)),
        ("created_at_ms", json!(0)),
    ] {
        let mut value_a = serde_json::to_value(&good).unwrap();
        value_a[field] = value;
        let changed: HostedStagingAuthority = serde_json::from_value(value_a).unwrap();
        assert!(changed.validate_for_dispatch(2_000).is_err(), "{field}");
    }
    for field in [
        "target_environment",
        "gitops_repository",
        "kustomization_path",
        "production_approval",
    ] {
        let mut value = serde_json::to_value(&good).unwrap();
        value[field] = json!("production");
        assert!(serde_json::from_value::<HostedStagingAuthority>(value).is_err());
    }
    let other = authority(true);
    assert!(p.validate(&other).is_err());
    let mut changed = good.clone();
    changed.build_evidence_hash = format!("sha256:{}", "9".repeat(64));
    assert!(p.validate(&changed).is_err());
}

#[test]
fn staging_plan_rejects_configuration_changes_mutable_baselines_and_no_ops() {
    let a = authority(false);
    let good = plan(&a, false);
    let mut changed = good.clone();
    changed.updated_content = changed
        .updated_content
        .replace("namespace: apps-staging", "namespace: apps");
    assert!(changed.validate(&a).is_err());
    let mut changed = good.clone();
    changed.original_content = changed
        .original_content
        .replace("namespace: apps-staging", "namespace: apps");
    changed.updated_content = update_kustomization_image(
        &changed.original_content,
        a.coordinates().unwrap().1,
        &a.image_ref().unwrap(),
    )
    .unwrap();
    assert!(changed.validate(&a).is_err());
    let mut changed = good.clone();
    changed.original_content = changed.updated_content.clone();
    assert!(changed.validate(&a).is_err());
    let mut changed = good.clone();
    changed.base_commit_sha = "main".into();
    assert!(changed.validate(&a).is_err());
    let mut changed = good.clone();
    changed
        .original_content
        .push_str(&" ".repeat(MAX_FILE_BYTES));
    assert!(changed.validate(&a).is_err());
    let mut changed = good.clone();
    changed.original_content = changed.original_content.replace("digest:", "newTag:");
    assert!(changed.validate(&a).is_err());
    let mut changed = good.clone();
    changed.original_content = "images: [".into();
    assert!(changed.validate(&a).is_err());
    let mut changed = good.clone();
    changed
        .updated_content
        .push_str("\n# Not the approved scalar edit\n");
    assert!(changed.validate(&a).is_err());
}
