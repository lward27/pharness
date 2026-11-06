use crate::campus_journal::test_support::{profile, student, ISSUER, TENANT};
use crate::{
    AdmitCampusLabLaunch, CampusLabAdmission, CampusLabLimits, CampusLabRejection,
    CampusLabSlotHoldKind, SqliteStore,
};
use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};
use sqlx::{Row, SqlitePool};
use std::borrow::Cow;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

const MIGRATION: &str = include_str!("../../../../migrations/0061_campus_teardown.sql");

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
        "pharness-campus-0061-{label}-{}-{}-{}.db",
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

async fn all_rows(pool: &SqlitePool, tables: &[String]) -> BTreeMap<String, Vec<String>> {
    let mut rows = BTreeMap::new();
    for table in tables {
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
        let values: Vec<String> =
            sqlx::query_scalar(&format!("SELECT {select} FROM \"{table}\" ORDER BY 1"))
                .fetch_all(pool)
                .await
                .unwrap();
        rows.insert(table.clone(), values);
    }
    rows
}

async fn latest(pool: &SqlitePool) -> i64 {
    sqlx::query_scalar("SELECT MAX(version) FROM _sqlx_migrations")
        .fetch_one(pool)
        .await
        .unwrap()
}

/// Upgrade to exactly schema 61. Later migrations have their own tests.
async fn at_schema_61(path: &Path) -> SqliteStore {
    let pool = pool_at(path).await;
    migrator_through(61).run(&pool).await.unwrap();
    sqlx::query("PRAGMA foreign_keys = ON")
        .execute(&pool)
        .await
        .unwrap();
    SqliteStore { pool }
}

fn launch(session: &str, subject: &str, now: i64) -> AdmitCampusLabLaunch {
    AdmitCampusLabLaunch {
        tenant_id: TENANT.into(),
        course_id: "course-101".into(),
        assignment_id: "lab-1".into(),
        issuer: ISSUER.into(),
        subject: subject.into(),
        idempotency_key: None,
        reset_from_session_id: None,
        candidate_lab_id: format!("lab-{session}"),
        candidate_budget_id: format!("budget-{session}"),
        candidate_session_id: session.into(),
        profile: profile(),
        effective_policy_hash: "sha256:policy".into(),
        expires_at: 9_000,
        actor: format!("student:{subject}"),
        now,
        limits: CampusLabLimits {
            lab_slots: 2,
            max_active_per_identity: 1,
            max_active_per_course: 5,
        },
    }
}

/// A database at schema 60 with two slots and an admitted lab session.
async fn schema_60(path: &Path) -> SqliteStore {
    let pool = pool_at(path).await;
    migrator_through(60).run(&pool).await.unwrap();
    // Admission reads slot holds, which this migration adds. A
    // connection-local TEMP table stands in for them: it is not part of the
    // main schema the test compares and is gone once the pool closes.
    sqlx::query("CREATE TEMP TABLE campus_lab_slot_holds (slot_id TEXT, released_at INTEGER)")
        .execute(&pool)
        .await
        .unwrap();
    let store = SqliteStore { pool };
    for subject in ["alice", "bob"] {
        student(&store, subject).await;
    }
    for slot in ["slot-1", "slot-2"] {
        store
            .register_campus_lab_slot(slot, TENANT, 100)
            .await
            .unwrap();
    }
    let admission = store
        .admit_campus_lab_launch(launch("session", "alice", 200))
        .await
        .unwrap();
    assert!(matches!(admission, CampusLabAdmission::Created(_)));
    store
}

#[tokio::test]
async fn schema_60_data_survives_0061_and_the_previous_release_refuses_it() {
    let database = path("upgrade");
    let old = schema_60(&database).await;
    let old_tables = tables(&old.pool).await;
    let before_rows = all_rows(&old.pool, &old_tables).await;
    let before_schema = schema(&old.pool).await;
    old.pool.close().await;

    let upgraded = at_schema_61(&database).await;
    assert_eq!(latest(&upgraded.pool).await, 61);
    assert_eq!(all_rows(&upgraded.pool, &old_tables).await, before_rows);
    let after_schema = schema(&upgraded.pool).await;
    for (object, sql) in &before_schema {
        assert_eq!(after_schema.get(object), Some(sql), "{object:?} changed");
    }
    let mut added = after_schema
        .keys()
        .filter(|object| !before_schema.contains_key(*object))
        .map(|(kind, name)| format!("{kind}:{name}"))
        .collect::<Vec<_>>();
    added.sort();
    assert_eq!(
        added,
        [
            "index:campus_lab_slot_holds_active",
            "table:campus_lab_slot_holds",
            "table:campus_teardown_hook_runs",
            "trigger:campus_lab_slot_holds_no_delete",
            "trigger:campus_lab_slot_holds_release_once",
            "trigger:campus_teardown_hook_runs_immutable",
            "trigger:campus_teardown_hook_runs_no_delete",
        ]
    );
    assert!(sqlx::query("PRAGMA foreign_key_check")
        .fetch_all(&upgraded.pool)
        .await
        .unwrap()
        .is_empty());

    // Pre-existing slots can be held, and a held free slot leaves the
    // tenant's capacity: the pre-existing session takes the only other one.
    sqlx::migrate!("./migrations")
        .run(&upgraded.pool)
        .await
        .unwrap();
    assert!(upgraded
        .hold_campus_lab_slot(
            "slot-2",
            TENANT,
            CampusLabSlotHoldKind::Quarantined,
            "leftovers",
            "operator",
            300,
        )
        .await
        .unwrap()
        .is_some());
    let admission = upgraded
        .admit_campus_lab_launch(launch("second", "bob", 300))
        .await
        .unwrap();
    assert!(matches!(
        admission,
        CampusLabAdmission::Rejected(CampusLabRejection::NoSlotsAvailable)
    ));

    assert!(matches!(
        migrator_through(60).run(&upgraded.pool).await,
        Err(sqlx::migrate::MigrateError::VersionMissing(61))
    ));
    upgraded.pool.close().await;
    remove(&database);
}

#[tokio::test]
async fn an_interrupted_0061_rolls_back_and_can_be_retried() {
    let database = path("interrupted");
    let old = schema_60(&database).await;
    let old_tables = tables(&old.pool).await;
    let before_rows = all_rows(&old.pool, &old_tables).await;
    let before_schema = schema(&old.pool).await;
    old.pool.close().await;
    let pool = pool_at(&database).await;
    let mut transaction = pool.begin().await.unwrap();
    sqlx::raw_sql(MIGRATION)
        .execute(&mut *transaction)
        .await
        .unwrap();
    transaction.rollback().await.unwrap();
    assert_eq!(schema(&pool).await, before_schema);
    assert_eq!(all_rows(&pool, &old_tables).await, before_rows);
    assert_eq!(latest(&pool).await, 60);
    pool.close().await;

    let retried = at_schema_61(&database).await;
    assert_eq!(latest(&retried.pool).await, 61);
    assert_eq!(all_rows(&retried.pool, &old_tables).await, before_rows);
    retried.pool.close().await;
    remove(&database);
}
