use super::*;
use serde_json::json;

mod migration;

async fn seed(store: &SqliteStore, hosted: bool) {
    sqlx::query(
        "INSERT INTO work_items(id,status,title,intent,source_repo,source_ref,target_environment,
        max_attempts,max_elapsed_seconds,created_at,updated_at,status_changed_at,mode,
        workflow_policy_json,workflow_policy_hash)
        VALUES('w','submitted','Fixture','Bounded change','https://github.com/example/app.git',
          'main','repository',2,3600,'1','1','1','repo',?1,?2)",
    )
    .bind(
        hosted
            .then(|| json!({"schema_version":"pharness.dev/hosted-workflow/v1alpha1"}).to_string()),
    )
    .bind(hosted.then_some("sha256:fixture"))
    .execute(&store.pool)
    .await
    .unwrap();
    sqlx::raw_sql(
        "INSERT INTO sessions(id,title,cwd,created_at,updated_at) VALUES('s','Fixture','/tmp','1','1');
         INSERT INTO runs(id,session_id,status,user_task,max_turns,started_at) VALUES('r','s','completed','Fixture',4,'1');
         INSERT INTO work_plans(id,work_item_id,session_id,run_id,status,title,summary,risk_level,created_at)
           VALUES('wp','w','s','r','approved','Fixture','Fixture','low','1');
         INSERT INTO change_sets(id,work_item_id,work_plan_id,session_id,run_id,status,title,summary,risk_level,material_hash,created_at)
           VALUES('c','w','wp','s','r','approved','Fixture','Fixture','low','sha256:fixture','1');
         INSERT INTO pipeline_intents(id,change_set_id,work_plan_id,session_id,run_id,status,title,summary,risk_level,intent_kind,created_at)
           VALUES('p','c','wp','s','r','completed','Fixture','Fixture','low','tekton','1');
         INSERT INTO artifacts(id,session_id,run_id,kind,label,created_at,content_json)
           VALUES('a','s','r','gitops_update_plan','Fixture','1','{}');",
    ).execute(&store.pool).await.unwrap();
}

fn deployment(id: &str, stage: DeliveryStage) -> CreateDeploymentIntent {
    CreateDeploymentIntent {
        id: id.into(),
        pipeline_intent_id: "p".into(),
        change_set_id: "c".into(),
        work_plan_id: "wp".into(),
        remediation_plan_id: None,
        incident_id: None,
        session_id: SessionId::new("s"),
        run_id: Some(RunId::new("r")),
        status: "proposed".into(),
        title: id.into(),
        summary: "Fixture".into(),
        risk_level: "low".into(),
        intent_kind: "gitops_deploy".into(),
        target_environment: Some(stage.as_str().into()),
        target_namespace: Some(
            if stage == DeliveryStage::Production {
                "apps"
            } else {
                "apps-staging"
            }
            .into(),
        ),
        argo_application: Some(id.into()),
        resource_namespace: None,
        resource_kind: None,
        resource_name: None,
        intent_json: json!({"build_digest":"sha256:verified"}),
    }
}

fn gitops(id: &str, deployment: &str) -> CreateGitOpsChangeSet {
    CreateGitOpsChangeSet {
        id: id.into(),
        work_item_id: "w".into(),
        work_plan_id: "wp".into(),
        source_change_set_id: "c".into(),
        pipeline_intent_id: "p".into(),
        deployment_intent_id: deployment.into(),
        gitops_update_plan_artifact_id: "a".into(),
        session_id: SessionId::new("s"),
        run_id: Some(RunId::new("r")),
        status: "proposed".into(),
        title: id.into(),
        summary: "Fixture".into(),
        risk_level: "low".into(),
        material_hash: "sha256:fixture".into(),
        gitops_repo: "https://github.com/example/gitops.git".into(),
        gitops_ref: "main".into(),
        head_branch: id.into(),
        kustomization_path: format!("{deployment}/kustomization.yaml"),
        image_name: "registry.example/app".into(),
        image_ref: "registry.example/app@sha256:verified".into(),
        gitops_change_set_json: json!({"retained":"original evidence"}),
    }
}

async fn release(store: &SqliteStore, id: &str, deployment: &str) {
    sqlx::query("INSERT INTO releases(id,deployment_intent_id,pipeline_intent_id,change_set_id,work_plan_id,
        session_id,run_id,status,title,summary,risk_level,release_kind,created_at,image_digest)
        VALUES(?1,?2,'p','c','wp','s','r','proposed','Fixture','Fixture','low','gitops_release','1','sha256:verified')")
        .bind(id).bind(deployment).execute(&store.pool).await.unwrap();
    sqlx::query("INSERT INTO registry_evidence(id,release_id,deployment_intent_id,pipeline_intent_id,change_set_id,
        work_plan_id,session_id,run_id,status,title,summary,risk_level,source,verification_status,created_at,image_digest)
        VALUES(?1,?2,?3,'p','c','wp','s','r','proposed','Fixture','Fixture','low','registry','verified','1','sha256:verified')")
        .bind(format!("registry_{id}")).bind(id).bind(deployment).execute(&store.pool).await.unwrap();
}

#[tokio::test]
async fn one_build_retains_separate_stage_deployments_gitops_and_release_evidence() {
    let store = SqliteStore::connect_in_memory().await.unwrap();
    seed(&store, true).await;
    for (stage, id) in [
        (DeliveryStage::Staging, "staging"),
        (DeliveryStage::Production, "production"),
    ] {
        let created = store
            .create_deployment_intent_for_stage(deployment(id, stage), stage)
            .await
            .unwrap();
        assert_eq!(created.delivery_stage, stage);
        assert_eq!(
            store
                .get_deployment_intent_for_stage("p", stage)
                .await
                .unwrap(),
            Some(created)
        );
        let change = store
            .create_gitops_change_set(gitops(&format!("g_{id}"), id))
            .await
            .unwrap();
        assert_eq!(
            store
                .get_gitops_change_set_by_deployment_intent(id)
                .await
                .unwrap(),
            Some(change)
        );
        release(&store, &format!("rel_{id}"), id).await;
        assert!(store
            .create_deployment_intent_for_stage(deployment("duplicate", stage), stage)
            .await
            .is_err());
        assert!(store
            .create_gitops_change_set(gitops("duplicate", id))
            .await
            .is_err());
    }
    let deliveries = store
        .list_deployment_intents(DeploymentIntentListFilter {
            pipeline_intent_id: Some("p".into()),
            limit: 20,
            ..Default::default()
        })
        .await
        .unwrap();
    assert_eq!(deliveries.len(), 2);
    let changes = store
        .list_gitops_change_sets(GitOpsChangeSetListFilter {
            pipeline_intent_id: Some("p".into()),
            limit: 20,
            ..Default::default()
        })
        .await
        .unwrap();
    assert_eq!(changes.len(), 2);
    assert!(store
        .get_deployment_intent_by_pipeline_intent("p")
        .await
        .unwrap()
        .is_none());
    assert!(store
        .get_gitops_change_set_by_pipeline_intent("p")
        .await
        .unwrap()
        .is_none());
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM pipeline_intents")
            .fetch_one(&store.pool)
            .await
            .unwrap(),
        1
    );
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT COUNT(DISTINCT image_digest) FROM releases")
            .fetch_one(&store.pool)
            .await
            .unwrap(),
        1
    );
    for id in ["staging", "production"] {
        let r = store
            .get_release_by_deployment_intent(id)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(r.pipeline_intent_id, "p");
        assert_eq!(r.status, "proposed"); // A separate record proves no deployment or approval.
        assert!(store
            .get_registry_evidence(&format!("registry_rel_{id}"))
            .await
            .unwrap()
            .is_some());
    }
    assert!(sqlx::query("PRAGMA foreign_key_check")
        .fetch_all(&store.pool)
        .await
        .unwrap()
        .is_empty());
}

#[tokio::test]
async fn stages_cannot_cross_workflow_contracts_or_rebind_targets_and_source() {
    let store = SqliteStore::connect_in_memory().await.unwrap();
    seed(&store, true).await;
    assert!(store
        .create_deployment_intent(deployment("legacy", DeliveryStage::Legacy))
        .await
        .is_err());
    let mut bad = deployment("bad", DeliveryStage::Staging);
    bad.target_environment = Some("production".into());
    assert!(store
        .create_deployment_intent_for_stage(bad, DeliveryStage::Staging)
        .await
        .is_err());
    let mut bad = deployment("bad", DeliveryStage::Staging);
    bad.run_id = None;
    assert!(store
        .create_deployment_intent_for_stage(bad, DeliveryStage::Staging)
        .await
        .is_err());
    store
        .create_deployment_intent_for_stage(
            deployment("d", DeliveryStage::Staging),
            DeliveryStage::Staging,
        )
        .await
        .unwrap();
    for update in [
        "delivery_stage='production'",
        "target_namespace='apps'",
        "argo_application='production'",
        "pipeline_intent_id='other'",
    ] {
        assert!(sqlx::query(&format!(
            "UPDATE deployment_intents SET {update} WHERE id='d'"
        ))
        .execute(&store.pool)
        .await
        .is_err());
    }
    let mut bad = gitops("bad", "d");
    bad.pipeline_intent_id = "other".into();
    assert!(store.create_gitops_change_set(bad).await.is_err());
    store
        .create_gitops_change_set(gitops("g", "d"))
        .await
        .unwrap();
    assert!(
        sqlx::query("UPDATE gitops_change_sets SET deployment_intent_id='other' WHERE id='g'")
            .execute(&store.pool)
            .await
            .is_err()
    );

    let legacy = SqliteStore::connect_in_memory().await.unwrap();
    seed(&legacy, false).await;
    assert!(legacy
        .create_deployment_intent_for_stage(
            deployment("d", DeliveryStage::Staging),
            DeliveryStage::Staging
        )
        .await
        .is_err());
    legacy
        .create_deployment_intent(deployment("d", DeliveryStage::Legacy))
        .await
        .unwrap();
    legacy
        .create_gitops_change_set(gitops("g", "d"))
        .await
        .unwrap();
    assert_eq!(
        legacy
            .get_deployment_intent_by_pipeline_intent("p")
            .await
            .unwrap()
            .unwrap()
            .id,
        "d"
    );
    assert_eq!(
        legacy
            .get_gitops_change_set_by_pipeline_intent("p")
            .await
            .unwrap()
            .unwrap()
            .id,
        "g"
    );
}

#[tokio::test]
async fn hosted_delivery_uses_build_lineage_without_fabricating_a_coding_run() {
    let store = SqliteStore::connect_in_memory().await.unwrap();
    seed(&store, true).await;
    sqlx::query("UPDATE pipeline_intents SET run_id=NULL WHERE id='p'")
        .execute(&store.pool)
        .await
        .unwrap();
    let mut d = deployment("d", DeliveryStage::Staging);
    d.run_id = None;
    store
        .create_deployment_intent_for_stage(d, DeliveryStage::Staging)
        .await
        .unwrap();
    let mut g = gitops("g", "d");
    g.run_id = None;
    let created = store.create_gitops_change_set(g).await.unwrap();
    assert!(created.run_id.is_none());
    assert_eq!(
        store
            .get_gitops_change_set_by_deployment_intent("d")
            .await
            .unwrap(),
        Some(created)
    );
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM runs")
            .fetch_one(&store.pool)
            .await
            .unwrap(),
        1
    );
    assert!(sqlx::query("PRAGMA foreign_key_check")
        .fetch_all(&store.pool)
        .await
        .unwrap()
        .is_empty());
}
