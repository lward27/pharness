use super::*;
use crate::hosted_sdlc::HostedRollbackPermission;
use serde_json::json;

fn digest(c: char) -> String {
    format!("sha256:{}", c.to_string().repeat(64))
}

fn proposal(repository: &str) -> ProductionProposal {
    ProductionProposal {
        schema_version: PROPOSAL_SCHEMA.into(),
        work_item_id: "witem_finance".into(),
        operation_id: "workflowop_production".into(),
        pipeline_intent_id: "pipeline_original".into(),
        deployment_intent_id: "deployment_production".into(),
        workflow_policy_hash: digest('a'),
        application_repository: format!("https://github.com/lward27/{repository}.git"),
        source_commit_sha: "b".repeat(40),
        build_evidence_hash: digest('c'),
        image_digest: digest('d'),
        staging_evidence_artifact_id: "artifact_staging_runtime".into(),
        staging_evidence_hash: digest('e'),
        baseline_evidence_artifact_id: "artifact_production_baseline".into(),
        baseline_evidence_hash: digest('f'),
        previous_image_digest: digest('1'),
        production_configuration_hash: digest('2'),
        rollback_compatibility_hash: digest('3'),
        rollback: HostedRollbackPermission::OnePreviousVerifiedDeployment,
        created_at_ms: 1_000_000,
    }
}

fn plan(p: &ProductionProposal) -> ProductionGitOpsPlan {
    // The production layout has no namespace key. The native deployment reader
    // must still prove apps-prod, and the only editable bytes are the digest.
    let original = format!(
        "apiVersion: kustomize.config.k8s.io/v1beta1\nkind: Kustomization\nresources:\n  - deployment.yaml\nimages:\n  - name: {}\n    digest: {}\n",
        p.coordinates().unwrap().1, p.previous_image_digest
    );
    ProductionGitOpsPlan {
        schema_version: PLAN_SCHEMA.into(),
        proposal_hash: p.material_hash().unwrap(),
        base_commit_sha: "4".repeat(40),
        original_blob_sha: "5".repeat(40),
        updated_content: update_kustomization_image(
            &original,
            p.coordinates().unwrap().1,
            &p.image_ref().unwrap(),
        )
        .unwrap(),
        original_content: original,
    }
}

fn approval(p: &ProductionProposal, plan: &ProductionGitOpsPlan) -> HumanProductionApproval {
    HumanProductionApproval {
        schema_version: APPROVAL_SCHEMA.into(),
        work_item_id: p.work_item_id.clone(),
        operation_id: p.operation_id.clone(),
        proposal_hash: p.material_hash().unwrap(),
        gitops_plan_hash: plan.material_hash().unwrap(),
        operator_identity: "lucas".into(),
        reason: "Reviewed the exact candidate, production diff and healthy baseline".into(),
        approved_at_ms: 2_000_000,
        expires_at_ms: 2_000_000 + MAX_APPROVAL_MS,
    }
}

#[test]
fn each_finance_production_plan_preserves_the_reviewed_file_except_its_digest() {
    for repository in ["yfinance_wrapper", "finance-frontend"] {
        let p = proposal(repository);
        let plan = plan(&p);
        let decision = approval(&p, &plan);
        plan.validate(&p).unwrap();
        decision
            .validate_for_delivery(&p, &plan, 2_000_000)
            .unwrap();
        assert_eq!(
            plan.updated_content,
            plan.original_content
                .replace(&p.previous_image_digest, &p.image_digest)
        );
        assert!(p.coordinates().unwrap().0.starts_with("charts/"));
        assert!(!p.coordinates().unwrap().0.contains("staging"));
    }
}

#[test]
fn any_changed_production_material_requires_a_new_human_decision() {
    let p = proposal("yfinance_wrapper");
    let original_plan = plan(&p);
    let decision = approval(&p, &original_plan);
    for (field, replacement) in [
        ("work_item_id", json!("witem_other")),
        ("operation_id", json!("workflowop_other")),
        ("pipeline_intent_id", json!("pipeline_other")),
        ("deployment_intent_id", json!("deployment_other")),
        ("workflow_policy_hash", json!(digest('6'))),
        ("source_commit_sha", json!("6".repeat(40))),
        ("build_evidence_hash", json!(digest('6'))),
        ("image_digest", json!(digest('6'))),
        ("staging_evidence_artifact_id", json!("artifact_other")),
        ("staging_evidence_hash", json!(digest('6'))),
        ("baseline_evidence_artifact_id", json!("artifact_other")),
        ("baseline_evidence_hash", json!(digest('6'))),
        ("previous_image_digest", json!(digest('6'))),
        ("production_configuration_hash", json!(digest('6'))),
        ("rollback_compatibility_hash", json!(digest('6'))),
        ("rollback", json!("disabled")),
        ("created_at_ms", json!(1_000_001)),
        (
            "application_repository",
            json!("https://github.com/lward27/finance-frontend.git"),
        ),
    ] {
        let mut changed = json!(p);
        changed[field] = replacement;
        let changed: ProductionProposal = serde_json::from_value(changed).unwrap();
        let new_plan = plan(&changed);
        new_plan.validate(&changed).unwrap();
        assert!(
            decision
                .validate_for_delivery(&changed, &new_plan, 2_000_000)
                .is_err(),
            "{field}"
        );
    }
}

#[test]
fn changed_gitops_base_blob_or_complete_diff_invalidates_approval() {
    let p = proposal("yfinance_wrapper");
    let original = plan(&p);
    let decision = approval(&p, &original);
    let mut changed = original.clone();
    changed.base_commit_sha = "6".repeat(40);
    assert!(decision
        .validate_for_delivery(&p, &changed, 2_000_000)
        .is_err());
    changed = original.clone();
    changed.original_blob_sha = "6".repeat(40);
    assert!(decision
        .validate_for_delivery(&p, &changed, 2_000_000)
        .is_err());
    changed = original.clone();
    changed
        .original_content
        .push_str("# Concurrent configuration edit\n");
    changed
        .updated_content
        .push_str("# Concurrent configuration edit\n");
    changed.validate(&p).unwrap();
    assert!(decision
        .validate_for_delivery(&p, &changed, 2_000_000)
        .is_err());
}

#[test]
fn production_plan_rejects_other_edits_no_ops_mutable_images_and_wrong_baselines() {
    let p = proposal("yfinance_wrapper");
    let original = plan(&p);
    for suffix in [
        "namespace: apps-staging\n",
        "namespace: another\n",
        "resources: []\n",
    ] {
        let mut changed = original.clone();
        changed.original_content.push_str(suffix);
        changed.updated_content.push_str(suffix);
        assert!(changed.validate(&p).is_err());
    }
    for (from, to) in [
        ("deployment.yaml".to_string(), "another.yaml".to_string()),
        (p.image_digest.clone(), p.previous_image_digest.clone()),
        (p.image_digest.clone(), "latest".to_string()),
    ] {
        let mut changed = original.clone();
        changed.updated_content = changed.updated_content.replace(&from, &to);
        assert!(changed.validate(&p).is_err());
    }
    let mut changed = original;
    changed.original_content = changed
        .original_content
        .replace(&p.previous_image_digest, &digest('6'));
    assert!(changed.validate(&p).is_err());
    let mut no_op = p;
    no_op.previous_image_digest = no_op.image_digest.clone();
    assert!(no_op.validate().is_err());
}

#[test]
fn approval_keeps_its_original_thirty_minute_window_and_identity_on_replay() {
    let p = proposal("yfinance_wrapper");
    let plan = plan(&p);
    let decision = approval(&p, &plan);
    let original_hash = decision.material_hash().unwrap();
    for now in [
        decision.approved_at_ms,
        decision.approved_at_ms + 60_000,
        decision.expires_at_ms - 1,
    ] {
        decision.validate_for_delivery(&p, &plan, now).unwrap();
        assert_eq!(decision.material_hash().unwrap(), original_hash);
    }
    for now in [
        decision.approved_at_ms - 1,
        decision.expires_at_ms,
        i64::MAX,
    ] {
        assert!(decision.validate_for_delivery(&p, &plan, now).is_err());
    }
    for (field, value) in [
        ("approved_at_ms", json!(p.created_at_ms - 1)),
        ("expires_at_ms", json!(decision.approved_at_ms)),
        ("expires_at_ms", json!(decision.expires_at_ms + 1)),
        ("operator_identity", json!("controller:hosted-workflow")),
        ("operator_identity", json!("worker:finance")),
        ("operator_identity", json!("agent:builder")),
        ("operator_identity", json!("")),
        ("operator_identity", json!(" lucas")),
        ("reason", json!(" ")),
    ] {
        let mut changed = json!(decision);
        changed[field] = value;
        let changed: HumanProductionApproval = serde_json::from_value(changed).unwrap();
        assert!(
            changed.validate_for_delivery(&p, &plan, 2_000_000).is_err(),
            "{field}"
        );
    }
}

#[test]
fn production_cannot_inherit_staging_or_source_authority_or_accept_undeclared_fields() {
    let p = proposal("yfinance_wrapper");
    let plan = plan(&p);
    let decision = approval(&p, &plan);
    for schema in [
        super::super::staging::AUTHORITY_SCHEMA,
        super::super::HOSTED_SOURCE_MERGE_SCHEMA,
    ] {
        let mut changed = decision.clone();
        changed.schema_version = schema.into();
        assert!(changed.validate_for_delivery(&p, &plan, 2_000_000).is_err());
    }
    let mut changed = json!(p);
    changed["production_approved"] = json!(true);
    assert!(serde_json::from_value::<ProductionProposal>(changed).is_err());
    let mut changed = json!(decision);
    changed["allow_expired"] = json!(true);
    assert!(serde_json::from_value::<HumanProductionApproval>(changed).is_err());
    for repository in [
        "https://github.com/lward27/lucas_engineering.git",
        "https://github.com/other/yfinance_wrapper.git",
    ] {
        let mut changed = p.clone();
        changed.application_repository = repository.into();
        assert!(changed.validate().is_err());
    }
}

fn rollback_authority(p: &ProductionProposal) -> HostedProductionRollbackAuthority {
    HostedProductionRollbackAuthority {
        schema_version: ROLLBACK_AUTHORITY_SCHEMA.into(),
        execution_id: "prollback_finance".into(),
        delivery_execution_id: "pexec_finance".into(),
        delivery_authority_hash: digest('7'),
        proposal: p.clone(),
        admitted_gitops_commit_sha: "8".repeat(40),
        trigger: RollbackTrigger::Release,
        failed_result_artifact_id: "artifact_production_runtime_result".into(),
        failed_result_sha256: digest('9'),
        created_at_ms: 3_000_000,
        expires_at_ms: 3_000_000 + MAX_ROLLBACK_MS,
    }
}

/// The file as the admitted production commit left it: candidate pinned.
fn released_file(p: &ProductionProposal) -> String {
    plan(p).updated_content
}

#[test]
fn rollback_plan_restores_exactly_the_recorded_previous_digest() {
    for repository in ["yfinance_wrapper", "finance-frontend"] {
        let p = proposal(repository);
        let authority = rollback_authority(&p);
        authority.validate_for_dispatch(3_000_001).unwrap();
        let rollback = ProductionRollbackPlan::from_observed(
            &authority,
            "4".repeat(40),
            "5".repeat(40),
            released_file(&p),
        )
        .unwrap();
        assert_eq!(rollback.original_content, released_file(&p));
        assert_eq!(rollback.updated_content, plan(&p).original_content);
        assert_eq!(
            authority.image_ref().unwrap(),
            format!("{}@{}", p.coordinates().unwrap().1, p.previous_image_digest)
        );
        let request = rollback.commit_request(&authority, 3_000_001).unwrap();
        let input = &request["variables"]["input"];
        assert_eq!(input["expectedHeadOid"], json!("4".repeat(40)));
        assert_eq!(input["clientMutationId"], json!("prollback_finance"));
        assert_eq!(
            input["fileChanges"]["additions"][0]["path"],
            json!(p.coordinates().unwrap().0)
        );
        let body = input["message"]["body"].as_str().unwrap();
        assert!(body.contains(&format!("Reverses: {}", "8".repeat(40))));
        assert!(body.contains("Trigger: release"));
        assert!(body.contains(&format!(
            "Failed-window: artifact_production_runtime_result {}",
            digest('9')
        )));
    }
}

/// GitHub's `CommittableBranch` input accepts `branchName` (or `id`); a
/// `refName` key is rejected before any commit is created, so every hosted
/// GitOps write shares one exact branch shape.
#[test]
fn every_production_commit_request_names_the_gitops_branch_the_way_github_accepts() {
    let p = proposal("yfinance_wrapper");
    let plan = plan(&p);
    let decision = approval(&p, &plan);
    let delivery = HostedProductionAuthority {
        schema_version: AUTHORITY_SCHEMA.into(),
        execution_id: "pexec_finance".into(),
        gitops_change_set_id: "gcs_production_finance".into(),
        approval_artifact_id: "artifact_production_approval".into(),
        proposal: p.clone(),
        approval: decision.clone(),
        created_at_ms: decision.approved_at_ms,
        expires_at_ms: decision.expires_at_ms,
    };
    let rollback_authority = rollback_authority(&p);
    let rollback = ProductionRollbackPlan::from_observed(
        &rollback_authority,
        "4".repeat(40),
        "5".repeat(40),
        released_file(&p),
    )
    .unwrap();
    let expected =
        json!({"repositoryNameWithOwner":"lward27/lucas_engineering","branchName":"main"});
    for request in [
        plan.commit_request(&delivery, decision.approved_at_ms + 1)
            .unwrap(),
        rollback
            .commit_request(&rollback_authority, 3_000_001)
            .unwrap(),
    ] {
        let branch = &request["variables"]["input"]["branch"];
        assert_eq!(*branch, expected);
        assert!(branch.get("refName").is_none());
    }
}
#[test]
fn rollback_refuses_a_drifted_file_or_one_that_no_longer_pins_the_candidate() {
    let p = proposal("yfinance_wrapper");
    let authority = rollback_authority(&p);
    // Still on the baseline: there is nothing to undo.
    assert!(ProductionRollbackPlan::from_observed(
        &authority,
        "4".repeat(40),
        "5".repeat(40),
        plan(&p).original_content,
    )
    .is_err());
    // Someone already moved production elsewhere.
    assert!(ProductionRollbackPlan::from_observed(
        &authority,
        "4".repeat(40),
        "5".repeat(40),
        released_file(&p).replace(&p.image_digest, &digest('6')),
    )
    .is_err());
    // Another edit landed on top of the released candidate.
    let valid = ProductionRollbackPlan::from_observed(
        &authority,
        "4".repeat(40),
        "5".repeat(40),
        released_file(&p),
    )
    .unwrap();
    for suffix in ["namespace: apps-staging\n", "resources: []\n"] {
        let mut changed = valid.clone();
        changed.original_content.push_str(suffix);
        changed.updated_content.push_str(suffix);
        assert!(changed.validate(&authority).is_err());
    }
    let mut changed = valid.clone();
    changed.updated_content = changed
        .updated_content
        .replace(&p.previous_image_digest, "latest");
    assert!(changed.validate(&authority).is_err());
    let mut changed = valid;
    changed.updated_content = changed.original_content.clone();
    assert!(changed.validate(&authority).is_err());
}

#[test]
fn rollback_authority_requires_permission_window_and_the_failed_window_it_undoes() {
    let p = proposal("yfinance_wrapper");
    let authority = rollback_authority(&p);
    authority.validate_identity().unwrap();
    for (field, value) in [
        ("schema_version", json!(AUTHORITY_SCHEMA)),
        ("execution_id", json!("pexec_finance")),
        ("execution_id", json!("")),
        ("delivery_authority_hash", json!("8".repeat(40))),
        ("admitted_gitops_commit_sha", json!("8".repeat(39))),
        ("failed_result_artifact_id", json!("bad id")),
        ("failed_result_sha256", json!("not-a-digest")),
        ("created_at_ms", json!(p.created_at_ms - 1)),
        ("expires_at_ms", json!(3_000_000)),
        ("expires_at_ms", json!(3_000_000 + MAX_ROLLBACK_MS + 1)),
    ] {
        let mut changed = json!(authority);
        changed[field] = value;
        let changed: HostedProductionRollbackAuthority = serde_json::from_value(changed).unwrap();
        assert!(changed.validate_identity().is_err(), "{field}");
    }
    let mut disabled = authority.clone();
    disabled.proposal.rollback = HostedRollbackPermission::Disabled;
    assert!(disabled.validate_identity().is_err());
    for now in [
        authority.created_at_ms - 1,
        authority.expires_at_ms,
        i64::MAX,
    ] {
        assert!(authority.validate_for_dispatch(now).is_err());
    }
    let mut extra = json!(authority);
    extra["verified"] = json!(true);
    assert!(serde_json::from_value::<HostedProductionRollbackAuthority>(extra).is_err());
    // A plan derived under one authority does not transfer to a changed one.
    let rollback = ProductionRollbackPlan::from_observed(
        &authority,
        "4".repeat(40),
        "5".repeat(40),
        released_file(&p),
    )
    .unwrap();
    let mut observe = authority;
    observe.trigger = RollbackTrigger::Observe;
    assert!(rollback.validate(&observe).is_err());
}
