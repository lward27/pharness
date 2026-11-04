use crate::{
    CampusCourseRole, CampusIdentityKey, ConsumeCampusLoginState, CreateCampusSession,
    PutCampusMembership, SqliteStore, UpsertCampusIdentity,
};

fn key(tenant: &str, subject: &str) -> CampusIdentityKey {
    CampusIdentityKey {
        tenant_id: tenant.into(),
        issuer: "https://idp.example.test".into(),
        subject: subject.into(),
    }
}

fn consumption(state_hash: &str, now: i64) -> ConsumeCampusLoginState {
    ConsumeCampusLoginState {
        state_hash: state_hash.into(),
        tenant_id: "tenant-a".into(),
        now,
        expires_at: now + 600,
    }
}

async fn identity(store: &SqliteStore, subject: &str, email: &str, now: i64) -> String {
    store
        .upsert_campus_identity(UpsertCampusIdentity {
            candidate_id: format!("cid_{subject}_{now}"),
            key: key("tenant-a", subject),
            email: Some(email.into()),
            display_name: None,
            now,
        })
        .await
        .unwrap()
        .id
}

/// A file-backed store, so that several pooled connections really run
/// concurrently. The files are removed on drop.
struct DiskStore {
    store: SqliteStore,
    path: std::path::PathBuf,
}

impl DiskStore {
    async fn new(label: &str) -> Self {
        let path = std::env::temp_dir().join(format!(
            "pharness-campus-{label}-{}-{}.db",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let store = SqliteStore::connect(&path).await.unwrap();
        Self { store, path }
    }
}

impl Drop for DiskStore {
    fn drop(&mut self) {
        for suffix in ["", "-wal", "-shm"] {
            let _ = std::fs::remove_file(format!("{}{suffix}", self.path.display()));
        }
    }
}

#[tokio::test]
async fn login_states_are_consumed_exactly_once_and_pruned_after_expiry() {
    let store = SqliteStore::connect_in_memory().await.unwrap();
    assert!(store
        .consume_campus_login_state(consumption("state-1", 100))
        .await
        .unwrap());
    assert!(
        !store
            .consume_campus_login_state(consumption("state-1", 101))
            .await
            .unwrap(),
        "a replay is reported"
    );
    assert!(store
        .consume_campus_login_state(consumption("state-2", 101))
        .await
        .unwrap());
    // Still a replay one second before the sealed cookie expires.
    assert!(!store
        .consume_campus_login_state(consumption("state-1", 699))
        .await
        .unwrap());
    // Once the cookie has expired the record is pruned; the callback rejects
    // the expired cookie before it would ever get here.
    assert!(store
        .consume_campus_login_state(consumption("state-1", 700))
        .await
        .unwrap());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn concurrent_consumption_of_one_state_succeeds_once() {
    let disk = DiskStore::new("consume").await;
    let tasks = (0..16)
        .map(|_| {
            let store = disk.store.clone();
            tokio::spawn(async move {
                store
                    .consume_campus_login_state(consumption("state-race", 100))
                    .await
                    .unwrap()
            })
        })
        .collect::<Vec<_>>();
    let mut fresh = 0;
    for task in tasks {
        if task.await.unwrap() {
            fresh += 1;
        }
    }
    assert_eq!(fresh, 1);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn concurrent_membership_puts_report_exactly_one_creation() {
    let disk = DiskStore::new("membership").await;
    let tasks = (0..16)
        .map(|index| {
            let store = disk.store.clone();
            tokio::spawn(async move {
                store
                    .put_campus_course_membership(PutCampusMembership {
                        key: key("tenant-a", "student"),
                        course_id: "cs101".into(),
                        role: if index % 2 == 0 {
                            CampusCourseRole::Student
                        } else {
                            CampusCourseRole::Instructor
                        },
                        granted_by_identity_id: format!("admin-{index}"),
                        now: index,
                    })
                    .await
                    .unwrap()
            })
        })
        .collect::<Vec<_>>();
    let mut created = 0;
    for task in tasks {
        if task.await.unwrap() {
            created += 1;
        }
    }
    assert_eq!(created, 1);
    assert_eq!(
        disk.store
            .list_campus_course_members("tenant-a", "cs101")
            .await
            .unwrap()
            .len(),
        1
    );
}

#[tokio::test]
async fn identity_key_is_the_tuple_and_email_is_display_only() {
    let store = SqliteStore::connect_in_memory().await.unwrap();
    let first = identity(&store, "sub-1", "old@example.test", 10).await;
    let again = identity(&store, "sub-1", "new@example.test", 20).await;
    assert_eq!(first, again);
    let other = identity(&store, "sub-2", "new@example.test", 30).await;
    assert_ne!(first, other);

    let other_tenant = store
        .upsert_campus_identity(UpsertCampusIdentity {
            candidate_id: "cid_other_tenant".into(),
            key: key("tenant-b", "sub-1"),
            email: Some("new@example.test".into()),
            display_name: None,
            now: 40,
        })
        .await
        .unwrap();
    assert_ne!(other_tenant.id, first);
}

#[tokio::test]
async fn sessions_expire_on_idle_absolute_and_revocation() {
    let store = SqliteStore::connect_in_memory().await.unwrap();
    let identity_id = identity(&store, "sub-1", "a@example.test", 0).await;
    store
        .create_campus_session(CreateCampusSession {
            id_hash: "session".into(),
            tenant_id: "tenant-a".into(),
            identity_id: identity_id.clone(),
            csrf_hash: "csrf".into(),
            now: 0,
            idle_expires_at: 100,
            absolute_expires_at: 250,
        })
        .await
        .unwrap();

    assert!(store
        .touch_campus_session("session", "tenant-b", 50, 100)
        .await
        .unwrap()
        .is_none());
    let peeked = store
        .get_active_campus_session("session", "tenant-a", 99)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        peeked.idle_expires_at, 100,
        "a read-only lookup does not extend"
    );
    assert!(store
        .get_active_campus_session("session", "tenant-b", 99)
        .await
        .unwrap()
        .is_none());
    let active = store
        .touch_campus_session("session", "tenant-a", 99, 100)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(active.identity.id, identity_id);
    assert_eq!(active.idle_expires_at, 199);
    let capped = store
        .touch_campus_session("session", "tenant-a", 198, 100)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(capped.idle_expires_at, 250);
    assert!(store
        .touch_campus_session("session", "tenant-a", 250, 100)
        .await
        .unwrap()
        .is_none());

    store
        .create_campus_session(CreateCampusSession {
            id_hash: "idle".into(),
            tenant_id: "tenant-a".into(),
            identity_id: identity_id.clone(),
            csrf_hash: "csrf".into(),
            now: 0,
            idle_expires_at: 100,
            absolute_expires_at: 1000,
        })
        .await
        .unwrap();
    assert!(store
        .touch_campus_session("idle", "tenant-a", 100, 100)
        .await
        .unwrap()
        .is_none());
    assert!(store
        .get_active_campus_session("idle", "tenant-a", 100)
        .await
        .unwrap()
        .is_none());

    store
        .create_campus_session(CreateCampusSession {
            id_hash: "revoked".into(),
            tenant_id: "tenant-a".into(),
            identity_id,
            csrf_hash: "csrf".into(),
            now: 0,
            idle_expires_at: 100,
            absolute_expires_at: 1000,
        })
        .await
        .unwrap();
    assert!(store.revoke_campus_session("revoked", 5).await.unwrap());
    assert!(!store.revoke_campus_session("revoked", 6).await.unwrap());
    assert!(store
        .touch_campus_session("revoked", "tenant-a", 6, 100)
        .await
        .unwrap()
        .is_none());
    assert!(store
        .get_active_campus_session("revoked", "tenant-a", 6)
        .await
        .unwrap()
        .is_none());
}

#[tokio::test]
async fn memberships_and_admins_are_tenant_scoped() {
    let store = SqliteStore::connect_in_memory().await.unwrap();
    let admin = identity(&store, "admin", "admin@example.test", 0).await;
    store
        .sync_campus_bootstrap_admins(
            "tenant-a",
            &[("https://idp.example.test".into(), "admin".into())],
            0,
        )
        .await
        .unwrap();
    assert!(store
        .is_campus_tenant_admin(&key("tenant-a", "admin"))
        .await
        .unwrap());
    assert!(!store
        .is_campus_tenant_admin(&key("tenant-b", "admin"))
        .await
        .unwrap());

    let created = store
        .put_campus_course_membership(PutCampusMembership {
            key: key("tenant-a", "student"),
            course_id: "cs101".into(),
            role: CampusCourseRole::Student,
            granted_by_identity_id: admin.clone(),
            now: 1,
        })
        .await
        .unwrap();
    assert!(created);
    let changed = store
        .put_campus_course_membership(PutCampusMembership {
            key: key("tenant-a", "student"),
            course_id: "cs101".into(),
            role: CampusCourseRole::Instructor,
            granted_by_identity_id: admin,
            now: 2,
        })
        .await
        .unwrap();
    assert!(!changed);
    assert_eq!(
        store
            .get_campus_course_role(&key("tenant-a", "student"), "cs101")
            .await
            .unwrap(),
        Some(CampusCourseRole::Instructor)
    );
    assert_eq!(
        store
            .get_campus_course_role(&key("tenant-b", "student"), "cs101")
            .await
            .unwrap(),
        None
    );
    assert!(!store
        .campus_course_exists("tenant-b", "cs101")
        .await
        .unwrap());
    assert_eq!(
        store
            .list_campus_course_members("tenant-a", "cs101")
            .await
            .unwrap()
            .len(),
        1
    );
    assert!(store
        .delete_campus_course_membership(&key("tenant-a", "student"), "cs101")
        .await
        .unwrap());
    assert!(store
        .list_campus_memberships_for_identity(&key("tenant-a", "student"))
        .await
        .unwrap()
        .is_empty());

    store
        .sync_campus_bootstrap_admins("tenant-a", &[], 3)
        .await
        .unwrap();
    assert!(!store
        .is_campus_tenant_admin(&key("tenant-a", "admin"))
        .await
        .unwrap());
}
