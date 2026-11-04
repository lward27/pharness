use crate::campus_journal::test_support::{profile, ISSUER, TENANT};
use crate::{
    CampusIdentityKey, CampusJournalError, CreateAuditEvent, CreateCampusLabSession, CreateRun,
    CreateSession, PutCampusMembership, SqliteStore, UpsertCampusIdentity,
};
use pharness_core::{CampusLabSessionState, RunId, SessionId};
use serde_json::json;
use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};
use sqlx::{Row, SqlitePool};
use std::borrow::Cow;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

const MIGRATION: &str = include_str!("../../../../migrations/0058_campus_session_journal.sql");
const NEW_TABLES: [&str; 5] = [
    "campus_session_transitions",
    "campus_lab_slots",
    "campus_sessions_journal",
    "campus_session_operations",
    "campus_session_events",
];

fn migrator_through(version: i64) -> sqlx::migrate::Migrator {
    let all = sqlx::migrate!("./migrations");
    sqlx::migrate::Migrator {
        migrations: Cow::Owned(
            all.iter()
                .filter(|migration| migration.version <= version)
                .cloned()
                .collect(),
        ),
        ignore_missing: false,
        locking: true,
        no_tx: false,
    }
}

fn path(label: &str) -> PathBuf {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    std::env::temp_dir().join(format!(
        "pharness-campus-0058-{label}-{}-{}-{}.db",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ))
}

fn remove(path: &Path) {
    for suffix in ["", "-wal", "-shm"] {
        let _ = std::fs::remove_file(format!("{}{suffix}", path.display()));
    }
}

async fn pool_at(path: &Path, create: bool) -> SqlitePool {
    SqlitePoolOptions::new()
        .max_connections(1)
        .connect_with(
            SqliteConnectOptions::new()
                .filename(path)
                .create_if_missing(create)
                .foreign_keys(false),
        )
        .await
        .unwrap()
}

/// A database at schema 57 holding SDLC history and LUC-117 Campus identity
/// data.
async fn schema_57(path: &Path) -> SqliteStore {
    let pool = pool_at(path, true).await;
    migrator_through(57).run(&pool).await.unwrap();
    let store = SqliteStore { pool };
    store
        .create_session(CreateSession {
            id: SessionId::new("sess-legacy"),
            title: "Legacy session".into(),
            cwd: "/work".into(),
        })
        .await
        .unwrap();
    store
        .create_run(CreateRun {
            id: RunId::new("run-legacy"),
            session_id: SessionId::new("sess-legacy"),
            user_task: "Keep this run".into(),
            cwd: "/work".into(),
            max_turns: 3,
            initial_status: "completed".into(),
            execution_target_json: json!({"target": "local"}),
        })
        .await
        .unwrap();
    sqlx::query(
        "INSERT INTO work_items(id,status,title,intent,acceptance_criteria_json,source_repo,
         source_ref,target_environment,production_impacting,max_attempts,max_elapsed_seconds,
         created_at,updated_at,status_changed_at,mode,state_version,workflow_policy_json,
         workflow_policy_hash)
         VALUES('witem-legacy','submitted','Legacy item','Bounded request','[\"one\"]',
         'https://github.com/example/app.git','main','repository',0,2,3600,'100','100','100',
         'repo',1,'{\"schema_version\":\"pharness.dev/hosted-workflow/v1alpha1\"}','sha256:fixture')",
    )
    .execute(&store.pool)
    .await
    .unwrap();
    store
        .create_audit_event(CreateAuditEvent {
            id: "audit-legacy".into(),
            kind: "run.completed".into(),
            actor: Some("operator".into()),
            resource_kind: "run".into(),
            resource_id: "run-legacy".into(),
            run_id: Some(RunId::new("run-legacy")),
            payload_json: json!({"kept": true}),
        })
        .await
        .unwrap();
    let key = CampusIdentityKey {
        tenant_id: TENANT.into(),
        issuer: ISSUER.into(),
        subject: "alice".into(),
    };
    let identity = store
        .upsert_campus_identity(UpsertCampusIdentity {
            candidate_id: "cid-alice".into(),
            key: key.clone(),
            email: Some("alice@example.test".into()),
            display_name: None,
            now: 100,
        })
        .await
        .unwrap();
    store
        .put_campus_course_membership(PutCampusMembership {
            key,
            course_id: "course-101".into(),
            role: crate::CampusCourseRole::Student,
            granted_by_identity_id: identity.id,
            now: 100,
        })
        .await
        .unwrap();
    store
}

async fn tables(pool: &SqlitePool) -> Vec<String> {
    sqlx::query_scalar(
        "SELECT name FROM sqlite_master WHERE type = 'table'
         AND name NOT LIKE 'sqlite_%' AND name != '_sqlx_migrations' ORDER BY name",
    )
    .fetch_all(pool)
    .await
    .unwrap()
}

/// Every row of every listed table, sorted, as JSON arrays of all columns.
async fn rows(pool: &SqlitePool, tables: &[String]) -> BTreeMap<String, Vec<String>> {
    let mut snapshot = BTreeMap::new();
    for table in tables {
        let columns = sqlx::query(&format!("PRAGMA table_info(\"{table}\")"))
            .fetch_all(pool)
            .await
            .unwrap()
            .iter()
            .map(|row| format!("\"{}\"", row.get::<String, _>("name")))
            .collect::<Vec<_>>()
            .join(",");
        let mut values: Vec<String> =
            sqlx::query_scalar(&format!("SELECT json_array({columns}) FROM \"{table}\""))
                .fetch_all(pool)
                .await
                .unwrap();
        values.sort();
        snapshot.insert(table.clone(), values);
    }
    snapshot
}

async fn schema(pool: &SqlitePool) -> BTreeMap<(String, String), Option<String>> {
    sqlx::query("SELECT type, name, sql FROM sqlite_master WHERE name NOT LIKE 'sqlite_%'")
        .fetch_all(pool)
        .await
        .unwrap()
        .iter()
        .map(|row| ((row.get("type"), row.get("name")), row.get("sql")))
        .collect()
}

async fn latest(pool: &SqlitePool) -> i64 {
    sqlx::query_scalar("SELECT MAX(version) FROM _sqlx_migrations")
        .fetch_one(pool)
        .await
        .unwrap()
}

#[tokio::test]
async fn schema_57_data_survives_0058_and_an_archive_restores_the_previous_release() {
    let database = path("upgrade");
    let archive = path("archive");
    let old = schema_57(&database).await;
    let old_tables = tables(&old.pool).await;
    let before_rows = rows(&old.pool, &old_tables).await;
    let before_schema = schema(&old.pool).await;
    assert!(before_rows["runs"].len() == 1 && before_rows["campus_identities"].len() == 1);
    assert_eq!(before_rows["hosted_reconciliations"].len(), 1);
    // The release script archives the live database before a migrating
    // release; VACUUM INTO is what `pharness-admin data archive` uses.
    sqlx::query("VACUUM INTO ?1")
        .bind(archive.display().to_string())
        .execute(&old.pool)
        .await
        .unwrap();
    old.pool.close().await;

    let upgraded = SqliteStore::connect(&database).await.unwrap();
    assert_eq!(latest(&upgraded.pool).await, 58);
    assert_eq!(rows(&upgraded.pool, &old_tables).await, before_rows);
    let after_schema = schema(&upgraded.pool).await;
    for (object, sql) in &before_schema {
        assert_eq!(after_schema.get(object), Some(sql), "{object:?} changed");
    }
    let added: Vec<_> = after_schema
        .keys()
        .filter(|object| !before_schema.contains_key(*object))
        .filter(|(kind, _)| kind == "table")
        .map(|(_, name)| name.as_str())
        .collect();
    let mut expected = NEW_TABLES.to_vec();
    expected.sort_unstable();
    let mut added_sorted = added.clone();
    added_sorted.sort_unstable();
    assert_eq!(added_sorted, expected);
    let new_rows = rows(
        &upgraded.pool,
        &NEW_TABLES.iter().map(|t| t.to_string()).collect::<Vec<_>>(),
    )
    .await;
    assert_eq!(new_rows["campus_session_transitions"].len(), 13);
    assert!(new_rows["campus_sessions_journal"].is_empty());
    assert!(sqlx::query("PRAGMA foreign_key_check")
        .fetch_all(&upgraded.pool)
        .await
        .unwrap()
        .is_empty());
    let integrity: String = sqlx::query_scalar("PRAGMA integrity_check")
        .fetch_one(&upgraded.pool)
        .await
        .unwrap();
    assert_eq!(integrity, "ok");

    // The pre-existing identity can launch a lab session immediately.
    upgraded
        .register_campus_lab_slot("slot-1", TENANT, 200)
        .await
        .unwrap();
    let (session, _) = upgraded
        .create_campus_lab_session(CreateCampusLabSession {
            session_id: "lab-1".into(),
            tenant_id: TENANT.into(),
            course_id: "course-101".into(),
            issuer: ISSUER.into(),
            subject: "alice".into(),
            profile: profile(),
            expires_at: 4_000,
            actor: "student:alice".into(),
            now: 200,
        })
        .await
        .unwrap();
    assert_eq!(session.state, CampusLabSessionState::Requested);
    assert!(matches!(
        upgraded
            .create_campus_lab_session(CreateCampusLabSession {
                session_id: "lab-2".into(),
                tenant_id: TENANT.into(),
                course_id: "course-101".into(),
                issuer: ISSUER.into(),
                subject: "mallory".into(),
                profile: profile(),
                expires_at: 4_000,
                actor: "student:mallory".into(),
                now: 200,
            })
            .await,
        Err(CampusJournalError::Invalid(_))
    ));
    assert_eq!(rows(&upgraded.pool, &old_tables).await, before_rows);

    // The previous release refuses the migrated database, so rollback is a
    // restore of the archive, never a downgrade in place.
    assert!(matches!(
        migrator_through(57).run(&upgraded.pool).await,
        Err(sqlx::migrate::MigrateError::VersionMissing(58))
    ));
    upgraded.pool.close().await;

    let restored = pool_at(&archive, false).await;
    migrator_through(57).run(&restored).await.unwrap();
    assert_eq!(latest(&restored).await, 57);
    assert_eq!(rows(&restored, &old_tables).await, before_rows);
    assert_eq!(schema(&restored).await, before_schema);
    restored.close().await;

    // Rolling forward again from the restored archive is the same upgrade.
    let reupgraded = SqliteStore::connect(&archive).await.unwrap();
    assert_eq!(latest(&reupgraded.pool).await, 58);
    assert_eq!(rows(&reupgraded.pool, &old_tables).await, before_rows);
    reupgraded.pool.close().await;
    remove(&database);
    remove(&archive);
}

#[tokio::test]
async fn an_interrupted_0058_rolls_back_and_can_be_retried() {
    let database = path("interrupted");
    let old = schema_57(&database).await;
    let old_tables = tables(&old.pool).await;
    let before_rows = rows(&old.pool, &old_tables).await;
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
    assert_eq!(tables(&old.pool).await, old_tables);
    assert_eq!(rows(&old.pool, &old_tables).await, before_rows);
    assert_eq!(latest(&old.pool).await, 57);
    old.pool.close().await;

    let retried = SqliteStore::connect(&database).await.unwrap();
    assert_eq!(latest(&retried.pool).await, 58);
    assert_eq!(rows(&retried.pool, &old_tables).await, before_rows);
    retried.pool.close().await;
    remove(&database);
}
