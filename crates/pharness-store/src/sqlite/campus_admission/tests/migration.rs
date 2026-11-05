use crate::campus_journal::test_support::{profile, student, ISSUER, TENANT};
use crate::{AdmitCampusLabLaunch, CampusLabAdmission, CampusLabLimits, SqliteStore};
use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};
use sqlx::{Row, SqlitePool};
use std::borrow::Cow;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

const MIGRATION: &str = include_str!("../../../../migrations/0059_campus_lab_admission.sql");
const NEW_TABLES: [&str; 3] = [
    "campus_labs",
    "campus_lab_generations",
    "campus_launch_requests",
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
        "pharness-campus-0059-{label}-{}-{}-{}.db",
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

async fn pool_at(path: &Path) -> SqlitePool {
    SqlitePoolOptions::new()
        .max_connections(1)
        .connect_with(
            SqliteConnectOptions::new()
                .filename(path)
                .create_if_missing(true)
                .foreign_keys(false),
        )
        .await
        .unwrap()
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

async fn schema(pool: &SqlitePool) -> BTreeMap<(String, String), Option<String>> {
    sqlx::query("SELECT type, name, sql FROM sqlite_master WHERE name NOT LIKE 'sqlite_%'")
        .fetch_all(pool)
        .await
        .unwrap()
        .into_iter()
        .map(|row| ((row.get("type"), row.get("name")), row.get("sql")))
        .collect()
}

async fn row_strings(pool: &SqlitePool, table: &str) -> Vec<String> {
    let columns: Vec<String> =
        sqlx::query_scalar(&format!("SELECT name FROM pragma_table_info('{table}')"))
            .fetch_all(pool)
            .await
            .unwrap();
    let select = columns
        .iter()
        .map(|column| format!("quote(\"{column}\")"))
        .collect::<Vec<_>>()
        .join(" || '|' || ");
    sqlx::query_scalar(&format!("SELECT {select} FROM \"{table}\" ORDER BY 1"))
        .fetch_all(pool)
        .await
        .unwrap()
}

async fn all_rows(pool: &SqlitePool, tables: &[String]) -> BTreeMap<String, Vec<String>> {
    let mut rows = BTreeMap::new();
    for table in tables {
        rows.insert(table.clone(), row_strings(pool, table).await);
    }
    rows
}

async fn latest(pool: &SqlitePool) -> i64 {
    sqlx::query_scalar("SELECT MAX(version) FROM _sqlx_migrations")
        .fetch_one(pool)
        .await
        .unwrap()
}

/// Upgrade to exactly schema 59, as the release that introduced 0059 did.
/// Later migrations have their own upgrade tests.
async fn at_schema_59(path: &Path) -> SqliteStore {
    let pool = pool_at(path).await;
    migrator_through(59).run(&pool).await.unwrap();
    sqlx::query("PRAGMA foreign_keys = ON")
        .execute(&pool)
        .await
        .unwrap();
    SqliteStore { pool }
}

/// A database at schema 58 with a lab session already in the journal.
async fn schema_58(path: &Path) -> SqliteStore {
    let pool = pool_at(path).await;
    migrator_through(58).run(&pool).await.unwrap();
    let store = SqliteStore { pool };
    student(&store, "alice").await;
    store
        .register_campus_lab_slot("slot-1", TENANT, 100)
        .await
        .unwrap();
    store
        .create_campus_lab_session(crate::CreateCampusLabSession {
            session_id: "journal-only".into(),
            tenant_id: TENANT.into(),
            course_id: "course-101".into(),
            issuer: ISSUER.into(),
            subject: "alice".into(),
            profile: profile(),
            expires_at: 5_000,
            actor: "student:alice".into(),
            now: 100,
        })
        .await
        .unwrap();
    store
}

#[tokio::test]
async fn schema_58_data_survives_0059_and_the_previous_release_refuses_it() {
    let database = path("upgrade");
    let old = schema_58(&database).await;
    let old_tables = tables(&old.pool).await;
    let before_rows = all_rows(&old.pool, &old_tables).await;
    let before_schema = schema(&old.pool).await;
    old.pool.close().await;

    let upgraded = at_schema_59(&database).await;
    assert_eq!(latest(&upgraded.pool).await, 59);
    assert_eq!(all_rows(&upgraded.pool, &old_tables).await, before_rows);
    let after_schema = schema(&upgraded.pool).await;
    for (object, sql) in &before_schema {
        assert_eq!(after_schema.get(object), Some(sql), "{object:?} changed");
    }
    let mut added = after_schema
        .keys()
        .filter(|object| !before_schema.contains_key(*object) && object.0 == "table")
        .map(|(_, name)| name.as_str())
        .collect::<Vec<_>>();
    added.sort_unstable();
    let mut expected = NEW_TABLES.to_vec();
    expected.sort_unstable();
    assert_eq!(added, expected);
    assert!(sqlx::query("PRAGMA foreign_key_check")
        .fetch_all(&upgraded.pool)
        .await
        .unwrap()
        .is_empty());

    // The journal-only session is not a lab session, but it still counts
    // against the tenant's single slot.
    assert!(upgraded
        .get_campus_lab_session_record("journal-only")
        .await
        .unwrap()
        .is_none());
    let admission = upgraded
        .admit_campus_lab_launch(AdmitCampusLabLaunch {
            tenant_id: TENANT.into(),
            course_id: "course-202".into(),
            assignment_id: "lab-1".into(),
            issuer: ISSUER.into(),
            subject: "alice".into(),
            idempotency_key: None,
            reset_from_session_id: None,
            candidate_lab_id: "lab".into(),
            candidate_budget_id: "budget".into(),
            candidate_session_id: "session".into(),
            profile: profile(),
            effective_policy_hash: "sha256:policy".into(),
            expires_at: 9_000,
            actor: "student:alice".into(),
            now: 200,
            limits: CampusLabLimits {
                lab_slots: 5,
                max_active_per_identity: 2,
                max_active_per_course: 5,
            },
        })
        .await
        .unwrap();
    assert!(matches!(admission, CampusLabAdmission::Rejected(_)));

    assert!(matches!(
        migrator_through(58).run(&upgraded.pool).await,
        Err(sqlx::migrate::MigrateError::VersionMissing(59))
    ));
    upgraded.pool.close().await;
    remove(&database);
}

#[tokio::test]
async fn an_interrupted_0059_rolls_back_and_can_be_retried() {
    let database = path("interrupted");
    let old = schema_58(&database).await;
    let old_tables = tables(&old.pool).await;
    let before_rows = all_rows(&old.pool, &old_tables).await;
    let mut transaction = old.pool.begin().await.unwrap();
    sqlx::raw_sql(MIGRATION)
        .execute(&mut *transaction)
        .await
        .unwrap();
    transaction.rollback().await.unwrap();
    assert_eq!(tables(&old.pool).await, old_tables);
    assert_eq!(all_rows(&old.pool, &old_tables).await, before_rows);
    assert_eq!(latest(&old.pool).await, 58);
    old.pool.close().await;

    let retried = at_schema_59(&database).await;
    assert_eq!(latest(&retried.pool).await, 59);
    assert_eq!(all_rows(&retried.pool, &old_tables).await, before_rows);
    retried.pool.close().await;
    remove(&database);
}
