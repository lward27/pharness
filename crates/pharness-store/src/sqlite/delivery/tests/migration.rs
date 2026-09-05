use super::*;
use sha2::{Digest, Sha256};
use std::borrow::Cow;

const MIGRATION: &str = include_str!("../../../../migrations/0054_delivery_stages.sql");

async fn original(path: &std::path::Path) -> SqliteStore {
    let pool = SqlitePoolOptions::new()
        .max_connections(1)
        .connect_with(
            SqliteConnectOptions::new()
                .filename(path)
                .create_if_missing(true)
                .foreign_keys(false),
        )
        .await
        .unwrap();
    let all = sqlx::migrate!("./migrations");
    sqlx::migrate::Migrator {
        migrations: Cow::Owned(all.iter().filter(|m| m.version <= 53).cloned().collect()),
        ignore_missing: false,
        locking: true,
        no_tx: false,
    }
    .run(&pool)
    .await
    .unwrap();
    let store = SqliteStore { pool };
    seed(&store, false).await;
    sqlx::query("INSERT INTO deployment_intents(id,pipeline_intent_id,change_set_id,work_plan_id,
        session_id,run_id,status,title,summary,risk_level,intent_kind,created_at,intent_json)
        VALUES('d','p','c','wp','s','r','approved','Historical','Preserve all fields','medium','argo_sync_deploy','100',
        '{\"original\":\"deployment evidence\"}')")
        .execute(&store.pool).await.unwrap();
    store
        .create_gitops_change_set(gitops("g", "d"))
        .await
        .unwrap();
    sqlx::query(
        "UPDATE gitops_change_sets SET revision=3, status_changed_by='original-owner',
        status_reason='original decision', status_changed_at='120' WHERE id='g'",
    )
    .execute(&store.pool)
    .await
    .unwrap();
    release(&store, "rel", "d").await;
    store
        .create_audit_event(CreateAuditEvent {
            id: "audit".into(),
            resource_kind: "gitops_change_set".into(),
            resource_id: "g".into(),
            kind: "gitops_change_set.proposed".into(),
            actor: Some("original-owner".into()),
            run_id: Some(RunId::new("r")),
            payload_json: json!({"original":true}),
        })
        .await
        .unwrap();
    store
}

fn path() -> std::path::PathBuf {
    std::env::temp_dir().join(format!(
        "pharness-delivery-migration-{}-{}.db",
        std::process::id(),
        super::super::super::now_string()
    ))
}

async fn snapshot(store: &SqliteStore) -> String {
    let mut digest = Sha256::new();
    for table in [
        "sessions",
        "runs",
        "work_items",
        "work_plans",
        "change_sets",
        "pipeline_intents",
        "deployment_intents",
        "gitops_change_sets",
        "releases",
        "registry_evidence",
        "artifacts",
        "audit_events",
    ] {
        let columns = sqlx::query(&format!("PRAGMA table_info({table})"))
            .fetch_all(&store.pool)
            .await
            .unwrap()
            .iter()
            .map(|r| r.get::<String, _>("name"))
            .filter(|n| n != "delivery_stage")
            .map(|n| format!("\"{n}\""))
            .collect::<Vec<_>>()
            .join(",");
        let rows: Vec<String> = sqlx::query_scalar(&format!(
            "SELECT json_array({columns}) FROM {table} ORDER BY id"
        ))
        .fetch_all(&store.pool)
        .await
        .unwrap();
        digest.update(table);
        digest.update(serde_json::to_vec(&rows).unwrap());
    }
    format!("{:x}", digest.finalize())
}

async fn clean(store: SqliteStore, path: &std::path::Path) {
    store.pool.close().await;
    for suffix in ["", "-wal", "-shm"] {
        let _ = std::fs::remove_file(format!("{}{suffix}", path.display()));
    }
}

#[tokio::test]
async fn schema_53_history_and_foreign_keys_survive_upgrade_and_reopen() {
    let path = path();
    let old = original(&path).await;
    let before = snapshot(&old).await;
    old.pool.close().await;
    let upgraded = SqliteStore::connect(&path).await.unwrap();
    assert_eq!(before, snapshot(&upgraded).await);
    let d = upgraded
        .get_deployment_intent_by_pipeline_intent("p")
        .await
        .unwrap()
        .unwrap();
    assert_eq!(d.delivery_stage, DeliveryStage::Legacy);
    assert_eq!(d.intent_json, json!({"original":"deployment evidence"}));
    let g = upgraded
        .get_gitops_change_set_by_pipeline_intent("p")
        .await
        .unwrap()
        .unwrap();
    assert_eq!(g.revision, 3);
    assert_eq!(g.status_changed_by.as_deref(), Some("original-owner"));
    assert_eq!(
        sqlx::query_scalar::<_, i64>("PRAGMA foreign_keys")
            .fetch_one(&upgraded.pool)
            .await
            .unwrap(),
        1
    );
    assert!(sqlx::query("PRAGMA foreign_key_check")
        .fetch_all(&upgraded.pool)
        .await
        .unwrap()
        .is_empty());
    assert!(upgraded
        .create_deployment_intent(deployment("duplicate", DeliveryStage::Legacy))
        .await
        .is_err());
    upgraded.pool.close().await;
    let reopened = SqliteStore::connect(&path).await.unwrap();
    assert_eq!(before, snapshot(&reopened).await);
    // SQLx rejects a newer applied migration even before hosted writes. The
    // rollback floor changes at schema deployment, not only at activation.
    let all = sqlx::migrate!("./migrations");
    let previous_reader = sqlx::migrate::Migrator {
        migrations: Cow::Owned(all.iter().filter(|m| m.version <= 53).cloned().collect()),
        ignore_missing: false,
        locking: true,
        no_tx: false,
    };
    assert!(matches!(
        previous_reader.run(&reopened.pool).await,
        Err(sqlx::migrate::MigrateError::VersionMissing(54))
    ));
    assert_eq!(before, snapshot(&reopened).await);
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT MAX(version) FROM _sqlx_migrations")
            .fetch_one(&reopened.pool)
            .await
            .unwrap(),
        54
    );
    clean(reopened, &path).await;
}

#[tokio::test]
async fn interrupted_migration_rolls_back_table_replacement_and_can_retry() {
    let path = path();
    let old = original(&path).await;
    let before = snapshot(&old).await;
    let mut transaction = old.pool.begin().await.unwrap();
    sqlx::raw_sql(MIGRATION)
        .execute(&mut *transaction)
        .await
        .unwrap();
    assert!(sqlx::query("INSERT INTO missing_table VALUES (1)")
        .execute(&mut *transaction)
        .await
        .is_err());
    transaction.rollback().await.unwrap();
    assert_eq!(before, snapshot(&old).await);
    assert!(sqlx::query("SELECT delivery_stage FROM deployment_intents")
        .fetch_all(&old.pool)
        .await
        .is_err());
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT MAX(version) FROM _sqlx_migrations")
            .fetch_one(&old.pool)
            .await
            .unwrap(),
        53
    );
    old.pool.close().await;
    let retried = SqliteStore::connect(&path).await.unwrap();
    assert_eq!(before, snapshot(&retried).await);
    clean(retried, &path).await;
}

#[tokio::test]
async fn existing_dangling_lineage_blocks_migration_without_discarding_data() {
    let path = path();
    let old = original(&path).await;
    sqlx::query("PRAGMA foreign_keys=OFF")
        .execute(&old.pool)
        .await
        .unwrap();
    sqlx::query("UPDATE gitops_change_sets SET run_id='missing-run' WHERE id='g'")
        .execute(&old.pool)
        .await
        .unwrap();
    let before = snapshot(&old).await;
    old.pool.close().await;
    assert!(SqliteStore::connect(&path).await.is_err());
    let preserved = SqliteStore::connect_read_only(&path).await.unwrap();
    assert_eq!(before, snapshot(&preserved).await);
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT MAX(version) FROM _sqlx_migrations")
            .fetch_one(&preserved.pool)
            .await
            .unwrap(),
        53
    );
    assert!(sqlx::query("SELECT delivery_stage FROM deployment_intents")
        .fetch_all(&preserved.pool)
        .await
        .is_err());
    clean(preserved, &path).await;
}
